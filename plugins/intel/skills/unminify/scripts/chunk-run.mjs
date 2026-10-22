import { timed, captureTimings } from './timing.mjs';
import { runSyntaxPreflight } from './syntax-preflight.mjs';
import { readFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { loadChunks, createChunkIgnore } from './chunk-manifest.mjs';
import { openChunkJournal, recoverChunkJournals } from './chunk-journal.mjs';
import { analyzeChunk, planChunk, renderChunkBatch, resolveChunkNames, applyChunkRenames, checkChunkRewrite, releaseChunkAnalysis } from './chunks.mjs';
import { analyzeImportChunk, resolveImportChunkNames, applyImportChunkRenames, checkImportChunkRewrite, releaseImportChunk } from './import-chunks.mjs';
import { reconsiderNames } from './collision.mjs';
import { iterateBatches } from './pack.mjs';
import { transformSyntax } from './syntax/runner.mjs';
import { mergeExternalImports, importMergeVersion } from './merge-imports.mjs';
import { syntaxFingerprint } from './syntax/stage.mjs';
import { hash } from './journal.mjs';
import { checkRewrite, checkRewriteAsync } from './checks.mjs';

// Drain every in-flight computation before propagating failure or closing its pool.
async function settle(tasks) {
  const results = await Promise.allSettled(tasks);
  const failure = results.find(result => result.status === 'rejected');
  if (failure) throw failure.reason;
}

/** Explicit chunks keep each stage and its group writes independently resumable. */
export async function createChunkRun(root, settings, ignores, { recover = true } = {}) {
  const manifest = settings.chunks ? await loadChunks(settings.chunks, root)
    : { chunks: [], files: new Set(), fingerprint: null };
  ignores ??= await createChunkIgnore(root, settings.ignoreFile);
  const ignored = new Set([...manifest.files].filter(ignores));
  const options = { manifest: manifest.fingerprint,
    ignored: [...ignored].sort(), chunkSiblings: !!settings.chunkSiblings,
    minSymbolLength: settings.minSymbolLength, maxInputTokens: settings.maxInputTokens, syntax: syntaxFingerprint, mergeImports: importMergeVersion };
  const syntaxOptions = { manifest: manifest.fingerprint, ignored: [...ignored].sort(),
    syntax: syntaxFingerprint, mergeImports: importMergeVersion };
  if (settings.chunks && recover) {
    recoverChunkJournals(root, syntaxOptions, 'syntax/chunks');
    recoverChunkJournals(root, settings.command === 'rename-bindings' || settings.command === 'all' ? id => {
      if (!manifest.chunks.some(chunk => chunk.id === id)) throw new Error(`chunk missing from manifest: ${id}`);
      return options;
    } : undefined);
    recoverChunkJournals(root, undefined, 'imports/chunks');
  }
  const renamePath = chunk => join(root, '.unminify', 'chunks', `${hash(chunk.id)}.jsonl`);
  const readSources = chunk => new Map(chunk.files.map(file => [file, readFileSync(join(root, file), 'utf8')]));
  async function restoreSources(inputs, fragmentContext, pool) {
    const baseline = new Map(inputs);
    await settle([...inputs].filter(([file]) => !ignored.has(file)).map(async ([file, source]) => {
      const options = { fragment: true, fragmentContext };
      const result = pool ? await pool.run({ kind: 'transform', source, options }) : transformSyntax(source, options);
      baseline.set(file, result.source);
    }));
    const options = { selected: new Set(inputs.keys()), ignored, chunkSiblings: true };
    return pool ? pool.run({ kind: 'merge', sources: baseline, options }) : mergeExternalImports(baseline, options);
  }
  const selected = new Set(), completed = new Map();
  let reporting;
  const driver = {
    files: manifest.files, jobs: [],
    more() {
      return [...completed.values()].filter(state => !state.readOnly && state.journal.done
        && state.chunk.files.some(file => selected.has(file) && !state.journal.selection?.includes(file)))
        .map(state => state.chunk);
    },
    addHistory(chunk, inputs, journal, ignores, options) {
      if (completed.has(chunk.id)) return;
      completed.set(chunk.id, { chunk, journal, inputs, sources: inputs, pending: false,
        selected: new Set(journal?.selection ?? []), ignored: new Set(chunk.files.filter(ignores)),
        chunkSiblings: options.chunkSiblings, readOnly: !journal });
    },
    select(files, fragments) {
      reporting ??= new Set(files);
      for (const file of files) selected.add(file);
      driver.jobs = manifest.chunks.filter(chunk => chunk.files.some(file => selected.has(file)));
      for (const file of manifest.files) fragments.add(file);
    },
    async syntaxPending({ fragmentContext, pool } = {}) {
      const pending = [];
      await settle(driver.jobs.map(async chunk => {
        const result = await runSyntaxPreflight(root, {scope: chunk.id, files: chunk.files, chunks: [chunk], ignored,
          fragmentContext, syntaxFingerprint, importMergeVersion}, async () => {
          const inputs = readSources(chunk), after = await restoreSources(inputs, fragmentContext, pool);
          return [...inputs].filter(([file, source]) => source !== after.get(file)).map(([file]) => file);
        });
        pending.push(...result.pending);
      }));
      return pending;
    },
    async restore({ emit, summary, sources, phase, fragmentContext, pool }) {
      let rejected = 0;
      await settle(driver.jobs.map(async chunk => {
        const inputs = readSources(chunk);
        for (const [file, source] of inputs) sources.set(file, source);
        if (existsSync(renamePath(chunk))) {
          emit({ chunk: chunk.id, status: 'syntax-skipped' });
          return;
        }
        const journal = openChunkJournal(root, chunk.id, inputs, syntaxOptions, 'syntax/chunks');
        if (journal.done) {
          if (journal.done.status === 'rejected') {
            rejected += chunk.files.filter(file => reporting.has(file)).length;
            emit({ chunk: chunk.id, status: 'rejected', reason: journal.done.reason });
          } else emit({ chunk: chunk.id, status: 'syntax-skipped' });
          return;
        }
        const endSyntax = phase('syntax', chunk.id);
        try {
          const after = await restoreSources(inputs, fragmentContext, pool);
          const reason = [...after].map(([, source]) => checkRewrite(source, source,
            { fragment: true, sharedProgram: true })).find(Boolean);
          if (reason) {
            journal.complete(inputs, 'rejected', reason);
            rejected += chunk.files.filter(file => reporting.has(file)).length;
            emit({ chunk: chunk.id, status: 'rejected', reason });
            return;
          }
          journal.commit(after, { status: 'syntax' });
          for (const [file, source] of after) sources.set(file, source);
          emit({ chunk: chunk.id, status: 'syntax', files: [...after].filter(([file, source]) => source !== inputs.get(file)).length });
        } finally { endSyntax(); }
      }));
      summary.rejected += rejected;
      return rejected;
    },
    prepareImports(sources, chunks = driver.jobs, refresh = false) {
      for (const chunk of chunks) {
        if (refresh) completed.delete(chunk.id);
        if (completed.has(chunk.id)) continue;
        const inputs = readSources(chunk);
        let journal, historyOptions;
        if (existsSync(renamePath(chunk))) {
          const header = JSON.parse(readFileSync(renamePath(chunk), 'utf8').split('\n')[0]);
          historyOptions = JSON.parse(header.options);
          journal = openChunkJournal(root, chunk.id, inputs, historyOptions);
          if (refresh && journal.done?.status === 'rejected') continue;
          if (!journal.done) {
            driver.addHistory(chunk, inputs, null, ignores, historyOptions);
            for (const [file, source] of inputs) sources.set(file, source);
            continue;
          }
        } else {
          historyOptions = { manifest: manifest.fingerprint, ignored: [...ignored].sort(), chunkSiblings: true };
          journal = openChunkJournal(root, chunk.id, inputs, historyOptions, 'imports/chunks');
          journal.begin(chunk.files.filter(file => selected.has(file)));
          if (!journal.done) journal.complete(inputs, 'imports');
        }
        driver.addHistory(chunk, inputs, journal, ignores, historyOptions);
        for (const [file, source] of inputs) sources.set(file, source);
      }
    },
    async process(chunk, context) {
      const { namer, withSlot, emit, summary, sources, phase, fragmentContext } = context;
      const api = { ...context.api };
      // Preserve caller-supplied checks; the built-in checker has an async path.
      const checkLocalRewrite = api.checkRewrite === checkRewrite ? checkRewriteAsync : api.checkRewrite;
      for (const name of ['planFile', 'renderBatch', 'resolveNames', 'applyRenames']) {
        api[name] = (...args) => timed(name, () => context.api[name](...args));
      }
      completed.delete(chunk.id);
      const inputs = new Map(chunk.files.map(file => [file, readFileSync(join(root, file), 'utf8')]));
      for (const file of chunk.files) {
        if (existsSync(join(root, '.unminify', `${file}.jsonl`))) throw new Error(`file rename journal already owns chunk member: ${file}`);
        sources.set(file, inputs.get(file));
      }
      const journal = openChunkJournal(root, chunk.id, inputs, options);
      const requested = chunk.files.filter(file => selected.has(file));
      const count = chunk.files.filter(file => reporting.has(file)).length;
      journal.begin(requested);
      const selection = new Set(journal.selection);
      if (journal.done) {
        summary.skipped += count;
        if (journal.done.status === 'rejected') summary.rejected += count;
        else completed.set(chunk.id, { chunk, journal, inputs, sources: inputs, pending: false, selected: selection, ignored, chunkSiblings: options.chunkSiblings });
        emit({ chunk: chunk.id, status: 'skipped', previous: journal.done.status, reason: journal.done.reason,
          ...(journal.done.file ? { file: journal.done.file, parseError: journal.done.parseError } : {}) });
        return;
      }
      const chunkOptions = { ...settings, selected: selection, ignored };
      const baseline = new Map(inputs);
      const analysis = timedAnalyzeChunk(baseline, chunkOptions);
      emit({ chunk: chunk.id, status: 'planned', names: analysis.candidates.length, frozen: analysis.statistics });
      const batches = timedPlanChunk(baseline, chunkOptions);
      const proposals = new Map();
      for (const batch of batches) if (journal.batches.has(batch.id)) {
        for (const [key, name] of Object.entries(journal.batches.get(batch.id))) proposals.set(key, name);
      }
      const replayed = timedResolveChunkNames(baseline, proposals, chunkOptions);
      const pending = batches.filter(batch => !journal.batches.has(batch.id));
      let batchFailed = false;
      async function request(packed, file, prefix = '') {
        const queuedAt = Date.now(), ids = packed.parts.map(({ batch }) => prefix + batch.id);
        let response;
        try { response = await namer.name(packed.payload, { file, chunk: chunk.id,
          batch_id: ids.length === 1 ? ids[0] : null, batch_ids: ids,
          name_count: Object.keys(packed.payload.placeholders).length, phase: 'model', queued_at: queuedAt }); }
        catch (error) {
          if (error.code === 'AUTH' || error.code === 'USAGE_LIMIT') throw error;
          batchFailed = true;
          summary.failedBatches += packed.parts.length;
          emit({ chunk: chunk.id, file, status: 'batch-failed', reason: error.message });
          return;
        }
        const keys = Object.keys(packed.payload.placeholders);
        if (!response.names || Object.keys(response.names).length !== keys.length || keys.some(key => typeof response.names[key] !== 'string')) throw new Error(`invalid names for chunk ${chunk.id}`);
        return packed.parts.map(({ batch, payload }) => [
          prefix + batch.id, new Map(Object.entries(payload.placeholders).map(([placeholder, key]) => [key, response.names[placeholder]])),
        ]);
      }
      async function requestBatches(pending, render, file, prefix = '', target = journal, concurrency = settings.concurrency) {
        const packed = iterateBatches(pending, render, settings);
        let finished = false;
        const replies = [];
        async function worker() {
          while (!finished) await withSlot(async () => {
            const next = timed('packBatches', () => packed.next());
            if (next.done) { finished = true; return; }
            const index = replies.length;
            replies.push(null);
            replies[index] = await request(next.value, file, prefix);
          });
        }
        try { await settle(Array.from({ length: concurrency }, worker)); }
        finally {
          packed.return();
          timed('journalBatchWrite', () => target.batchMany(replies.filter(Boolean).flat()));
        }
      }
      const endModel = phase('model', chunk.id);
      let after;
      try {
        await requestBatches(pending, (batch, placeholders) => timedRenderChunkBatch(baseline, batch, replayed, placeholders), chunk.id);
        if (batchFailed) return;
        // Response arrival order never decides collision suffixes or resume output.
        proposals.clear();
        for (const batch of batches) for (const [key, name] of Object.entries(journal.batches.get(batch.id))) proposals.set(key, name);
        const names = await reconsiderNames({ proposals, journal, id: 'program', namer: { name: (...args) => withSlot(() => namer.name(...args)) },
          maxInputTokens: settings.maxInputTokens, telemetry: { file: chunk.id, chunk: chunk.id },
          resolve: (values, onCollision) => timedResolveChunkNames(baseline, values, { ...chunkOptions, onCollision }) });
        journal.batch('program-resolved', names);
        after = timedApplyChunkRenames(baseline, names);
        const files = chunk.files.filter(file => !analysis.statistics.dynamicScope && selection.has(file) && !ignored.has(file));
        const results = new Array(files.length);
        let nextFile = 0, stopped = false;
        async function localWorker() {
          while (!stopped && nextFile < files.length) {
            const index = nextFile++, file = files[index];
            const source = after.get(file), local = new Map(), prefix = `local:${file}:`;
            // Each member sees its own replies immediately. Persist in member order
            // after draining, so response timing cannot change the journal.
            const writes = [];
            const localJournal = {
              batches: new Map(journal.batches), replies: new Map(journal.replies),
              batchMany(entries) {
                writes.push(() => journal.batchMany(entries));
                for (const [id, names] of entries) this.batches.set(id, Object.fromEntries(names));
              },
              response(id, names) {
                writes.push(() => journal.response(id, names));
                this.replies.set(id, Object.fromEntries(names));
              },
            };
            const result = results[index] = { writes, file };
            let stages;
            try {
              ({ stages } = api.planFile(source, { ...settings, fragment: true }));
              for (const stage of stages) {
                for (const batch of stage) if (localJournal.batches.has(prefix + batch.id)) {
                  for (const [key, name] of Object.entries(localJournal.batches.get(prefix + batch.id))) local.set(Number(key), name);
                }
                const localPending = stage.filter(batch => !localJournal.batches.has(prefix + batch.id));
                const replayedLocal = api.resolveNames(source, local, { fragment: true });
                await requestBatches(localPending, (batch, placeholders) => api.renderBatch(source, batch, replayedLocal, placeholders), file, prefix, localJournal, 1);
                if (batchFailed) { stopped = true; return; }
                for (const batch of stage) for (const [key, name] of Object.entries(localJournal.batches.get(prefix + batch.id))) local.set(Number(key), name);
              }
              const resolved = await reconsiderNames({ proposals: local, journal: localJournal, id: prefix, namer: { name: (...args) => withSlot(() => namer.name(...args)) },
                maxInputTokens: settings.maxInputTokens, telemetry: { file, chunk: chunk.id },
                resolve: (values, onCollision) => api.resolveNames(source, values, { fragment: true, onCollision }) });
              result.source = api.applyRenames(source, resolved);
              const recordTimings = captureTimings(), started = performance.now();
              try { result.reason = await checkLocalRewrite(source, result.source, {
                fragment: true, onParseError: message => { result.parseError = message; },
              }); }
              finally { recordTimings({ checkRewrite: performance.now() - started }); }
              if (result.reason) stopped = true;
            } catch (error) {
              result.error = error;
              stopped = true;
            } finally { if (stages) api.releasePlan?.(stages); }
          }
        }
        await settle(Array.from({ length: Math.min(settings.concurrency, files.length) }, localWorker));
        for (const result of results) if (result) for (const write of result.writes) {
          timed('journalBatchWrite', write);
        }
        const failure = results.find(result => result?.error);
        if (failure) throw failure.error;
        const rejected = results.find(result => result?.reason);
        if (rejected) {
          const details = { file: rejected.file, ...(rejected.parseError ? { parseError: rejected.parseError } : {}) };
          journal.complete(inputs, 'rejected', rejected.reason, details);
          summary.rejected += count;
          emit({ chunk: chunk.id, status: 'rejected', reason: rejected.reason, ...details });
          return;
        }
        if (batchFailed) return;
        for (const [index, file] of files.entries()) after.set(file, results[index].source);
      } finally { endModel(); }
      const endWrite = phase('check_write', chunk.id);
      try {
        const reason = timedCheckChunkRewrite(baseline, after);
        if (reason) {
          journal.complete(inputs, 'rejected', reason);
          summary.rejected += count;
          emit({ chunk: chunk.id, status: 'rejected', reason });
          return;
        }
        for (const [file, source] of after) sources.set(file, source);
        timed('write', () => journal.commit(after, { status: 'renamed' }));
        summary.completed += count;
        summary.chunkSiblingFiles = (summary.chunkSiblingFiles ?? 0) + [...after].filter(([file, source]) => !reporting.has(file) && source !== inputs.get(file)).length;
        emit({ chunk: chunk.id, status: 'renamed', files: count, batches: batches.length });
        completed.set(chunk.id, { chunk, journal, inputs: after, sources: after, pending: false, count, batches: batches.length, selected: selection, ignored, chunkSiblings: options.chunkSiblings });
      } finally {
        releaseChunkAnalysis(baseline);
        if (after) releaseChunkAnalysis(after);
        endWrite();
      }
    },
    async importOptions(journals, pool) {
      const renamedExports = new Set();
      for (const [file, journal] of journals) for (const names of journal.batches.values()) {
        for (const name of Object.values(names)) renamedExports.add(`${file}\0${name}`);
      }
      const chunkBindings = new Map(), owners = new Map(), plans = new Map(), analyses = new Map(), keys = new Map();
      for (const [id, state] of completed) {
        releaseChunkAnalysis(state.inputs);
        releaseChunkAnalysis(state.sources);
        const analysis = await analyzeImportChunk(state.sources, { selected: state.selected, ignored: state.ignored, chunkSiblings: state.chunkSiblings, allowImports: true }, pool);
        analyses.set(id, analysis);
        for (const file of state.chunk.files) keys.set(file, new Map());
        for (const [name, record] of analysis.bindings) for (const entry of record.declarations) keys.get(entry.file).set(entry.binding.key, name);
        for (const name of Object.values(state.journal?.batches.get('program-resolved') ?? {})) {
          const record = analysis.bindings.get(name);
          if (record) renamedExports.add(`${record.file}\0${name}`);
        }
        const bindings = new Map([...analysis.bindings].filter(([, record]) => record.declarations.length === 1)
          .map(([name, record]) => [name, { file: record.file, key: record.key }]));
        for (const file of state.chunk.files) { chunkBindings.set(file, bindings); owners.set(file, id); }
      }
      return { chunkBindings, renamedExports,
        release() {
          for (const analysis of analyses.values()) releaseImportChunk(analysis);
          analyses.clear(); keys.clear(); plans.clear();
        },
        groupFiles(file) {
          const id = owners.get(file);
          return id === undefined ? null : completed.get(id).chunk.files;
        },
        resolveGroup(grouped) {
          const first = grouped.keys().next().value, id = owners.get(first), state = completed.get(id);
          if (state.readOnly) return new Map([...grouped.keys()].map(file => [file, new Map()]));
          const analysis = analyses.get(id), all = new Map();
          for (const [file, proposals] of grouped) for (const [key, name] of proposals) {
            const byKey = keys.get(file);
            if (!byKey.has(key)) throw new Error(`unknown chunk import key: ${file}:${key}`);
            all.set(byKey.get(key), name);
          }
          const names = resolveImportChunkNames(analysis, all, { selected: state.selected, ignored: state.ignored,
            chunkSiblings: state.chunkSiblings, allowImports: true, restoreExternalImports: true });
          plans.set(id, names);
          const result = new Map([...grouped.keys()].map(file => [file, new Map()]));
          for (const [name, value] of names) {
            const binding = analysis.bindings.get(name);
            result.get(binding.file).set(binding.key, value);
          }
          return result;
        },
        async apply(sources, emit, summary) {
          for (const [id, state] of completed) {
            if (state.readOnly) continue;
            const names = plans.get(id) ?? new Map();
            const analysis = analyses.get(id);
            const after = await applyImportChunkRenames(state.sources, names, analysis, pool);
            const reason = await checkImportChunkRewrite(state.sources, after, analysis, pool);
            releaseImportChunk(analysis);
            analyses.delete(id);
            if (reason) {
              summary.rejected += state.chunk.files.filter(file => reporting.has(file)).length;
              if (state.pending) state.journal.complete(state.inputs, 'rejected', reason);
              for (const [file, source] of state.inputs) sources.set(file, source);
              emit({ chunk: id, status: 'imports-rejected', reason });
              continue;
            }
            if (state.pending || names.size) {
              state.journal.commit(after, { status: state.pending ? 'renamed' : 'imports' });
              if (state.pending) summary.completed += state.count;
              summary.chunkSiblingFiles = (summary.chunkSiblingFiles ?? 0) + [...after].filter(([file, source]) => !reporting.has(file) && source !== state.inputs.get(file)).length;
              emit({ chunk: id, status: state.pending ? 'renamed' : 'imports', files: state.count, batches: state.batches });
            }
            state.pending = false;
            state.inputs = after;
            state.sources = after;
            for (const [file, source] of after) sources.set(file, source);
          }
        },
      };
    },
  };
  return driver;
}

function timedAnalyzeChunk(...args) { return timed('analyzeChunk', () => analyzeChunk(...args)); }

function timedPlanChunk(...args) { return timed('planChunk', () => planChunk(...args)); }

function timedRenderChunkBatch(...args) { return timed('renderChunkBatch', () => renderChunkBatch(...args)); }

function timedResolveChunkNames(...args) { return timed('resolveChunkNames', () => resolveChunkNames(...args)); }

function timedApplyChunkRenames(...args) { return timed('applyChunkRenames', () => applyChunkRenames(...args)); }

function timedCheckChunkRewrite(...args) { return timed('checkChunkRewrite', () => checkChunkRewrite(...args)); }
