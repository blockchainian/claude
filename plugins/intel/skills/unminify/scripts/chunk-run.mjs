import { readFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { loadChunks, createChunkIgnore } from './chunk-manifest.mjs';
import { openChunkJournal, recoverChunkJournals } from './chunk-journal.mjs';
import { analyzeChunk, planChunk, renderChunkBatch, resolveChunkNames, applyChunkRenames, checkChunkRewrite } from './chunks.mjs';
import { reconsiderNames } from './collision.mjs';
import { packBatches } from './pack.mjs';
import { transformSyntax } from './syntax/runner.mjs';
import { mergeExternalImports, importMergeVersion } from './merge-imports.mjs';
import { syntaxFingerprint } from './syntax/stage.mjs';

/** Explicit chunks own their syntax, naming and writes as one transaction. */
export async function createChunkRun(root, settings) {
  const manifest = settings.chunks ? await loadChunks(settings.chunks, root)
    : { chunks: [], files: new Set(), fingerprint: null };
  const ignores = await createChunkIgnore(root, settings.ignoreFile);
  const ignored = new Set([...manifest.files].filter(ignores));
  const options = { manifest: manifest.fingerprint,
    ignored: [...ignored].sort(), chunkSiblings: !!settings.chunkSiblings,
    minSymbolLength: settings.minSymbolLength, maxInputTokens: settings.maxInputTokens, syntax: syntaxFingerprint, mergeImports: importMergeVersion };
  if (settings.chunks) recoverChunkJournals(root, id => {
    if (!manifest.chunks.some(chunk => chunk.id === id)) throw new Error(`chunk missing from manifest: ${id}`);
    return options;
  });
  const selected = new Set(), completed = new Map();
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
      for (const file of files) selected.add(file);
      driver.jobs = manifest.chunks.filter(chunk => chunk.files.some(file => selected.has(file)));
      for (const file of manifest.files) fragments.add(file);
    },
    async process(chunk, context) {
      const { api, namer, withSlot, emit, summary, sources, phase, fragmentContext } = context;
      completed.delete(chunk.id);
      const inputs = new Map(chunk.files.map(file => [file, readFileSync(join(root, file), 'utf8')]));
      for (const file of chunk.files) {
        if (existsSync(join(root, '.unminify', `${file}.jsonl`))) throw new Error(`file rename journal already owns chunk member: ${file}`);
        sources.set(file, inputs.get(file));
      }
      const journal = openChunkJournal(root, chunk.id, inputs, options);
      const requested = chunk.files.filter(file => selected.has(file));
      const count = requested.length;
      journal.begin(requested);
      const selection = new Set(journal.selection);
      if (journal.done) {
        summary.skipped += count;
        if (journal.done.status === 'rejected') summary.rejected += count;
        else completed.set(chunk.id, { chunk, journal, inputs, sources: inputs, pending: false, selected: selection, ignored, chunkSiblings: options.chunkSiblings });
        emit({ chunk: chunk.id, status: 'skipped', previous: journal.done.status, reason: journal.done.reason });
        return;
      }
      const chunkOptions = { ...settings, selected: selection, ignored };
      let baseline = new Map(inputs);
      const endSyntax = phase('syntax', chunk.id);
      try {
        for (const [file, source] of baseline) if (selection.has(file) && !ignored.has(file)) {
          baseline.set(file, transformSyntax(source, { fragment: true, fragmentContext }).source);
        }
      } finally { endSyntax(); }
      const endMerge = phase('selection_plan', chunk.id);
      try {
        const merged = mergeExternalImports(baseline, chunkOptions);
        emit({ chunk: chunk.id, status: 'merge-imports', files: [...merged].filter(([file, source]) => source !== baseline.get(file)).length });
        baseline = merged;
      } finally { endMerge(); }
      const analysis = analyzeChunk(baseline, chunkOptions);
      emit({ chunk: chunk.id, status: 'planned', names: analysis.candidates.length, frozen: analysis.statistics });
      const batches = planChunk(baseline, chunkOptions);
      const proposals = new Map();
      for (const batch of batches) if (journal.batches.has(batch.id)) {
        for (const [key, name] of Object.entries(journal.batches.get(batch.id))) proposals.set(key, name);
      }
      const replayed = resolveChunkNames(baseline, proposals, chunkOptions);
      const pending = batches.filter(batch => !journal.batches.has(batch.id));
      let batchFailed = false;
      async function request(packed, file, prefix = '') {
        const queuedAt = Date.now(), ids = packed.parts.map(({ batch }) => prefix + batch.id);
        return withSlot(async () => {
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
          for (const { batch, payload } of packed.parts) {
            journal.batch(prefix + batch.id, new Map(Object.entries(payload.placeholders).map(([placeholder, key]) => [key, response.names[placeholder]])));
          }
        });
      }
      const endModel = phase('model', chunk.id);
      let after;
      try {
        const packed = packBatches(pending, (batch, placeholders) => renderChunkBatch(baseline, batch, replayed, placeholders), settings);
        const results = await Promise.allSettled(packed.map(part => request(part, chunk.id)));
        const failure = results.find(result => result.status === 'rejected');
        if (failure) throw failure.reason;
        if (batchFailed) return;
        // Response arrival order never decides collision suffixes or resume output.
        proposals.clear();
        for (const batch of batches) for (const [key, name] of Object.entries(journal.batches.get(batch.id))) proposals.set(key, name);
        const names = await reconsiderNames({ proposals, journal, id: 'program', namer: { name: (...args) => withSlot(() => namer.name(...args)) },
          maxInputTokens: settings.maxInputTokens, telemetry: { file: chunk.id, chunk: chunk.id },
          resolve: (values, onCollision) => resolveChunkNames(baseline, values, { ...chunkOptions, onCollision }) });
        journal.batch('program-resolved', names);
        after = applyChunkRenames(baseline, names);
        for (const file of chunk.files.filter(file => !analysis.statistics.dynamicScope && selection.has(file) && !ignored.has(file))) {
          const source = after.get(file), local = new Map(), prefix = `local:${file}:`;
          let stages;
          try {
            ({ stages } = api.planFile(source, { ...settings, fragment: true }));
            for (const stage of stages) {
              for (const batch of stage) if (journal.batches.has(prefix + batch.id)) {
                for (const [key, name] of Object.entries(journal.batches.get(prefix + batch.id))) local.set(Number(key), name);
              }
              const localPending = stage.filter(batch => !journal.batches.has(prefix + batch.id));
              const replayedLocal = api.resolveNames(source, local, { fragment: true });
              const packedLocal = packBatches(localPending, (batch, placeholders) => api.renderBatch(source, batch, replayedLocal, placeholders), settings);
              const replies = await Promise.allSettled(packedLocal.map(part => request(part, file, prefix)));
              const failure = replies.find(reply => reply.status === 'rejected');
              if (failure) throw failure.reason;
              if (batchFailed) return;
              for (const batch of stage) for (const [key, name] of Object.entries(journal.batches.get(prefix + batch.id))) local.set(Number(key), name);
            }
            const resolved = await reconsiderNames({ proposals: local, journal, id: prefix, namer: { name: (...args) => withSlot(() => namer.name(...args)) },
              maxInputTokens: settings.maxInputTokens, telemetry: { file, chunk: chunk.id },
              resolve: (values, onCollision) => api.resolveNames(source, values, { fragment: true, onCollision }) });
            const rewritten = api.applyRenames(source, resolved);
            const reason = api.checkRewrite(source, rewritten, { fragment: true });
            if (reason) {
              journal.complete(inputs, 'rejected', reason);
              summary.rejected += count;
              emit({ chunk: chunk.id, status: 'rejected', reason });
              return;
            }
            after.set(file, rewritten);
          } finally { if (stages) api.releasePlan?.(stages); }
        }
      } finally { endModel(); }
      const endWrite = phase('check_write', chunk.id);
      try {
        const reason = checkChunkRewrite(baseline, after);
        if (reason) {
          journal.complete(inputs, 'rejected', reason);
          summary.rejected += count;
          emit({ chunk: chunk.id, status: 'rejected', reason });
          return;
        }
        for (const [file, source] of after) sources.set(file, source);
        completed.set(chunk.id, { chunk, journal, inputs, sources: after, pending: true, count, batches: batches.length, selected: selection, ignored, chunkSiblings: options.chunkSiblings });
      } finally { endWrite(); }
    },
    importOptions(journals) {
      const renamedExports = new Set();
      for (const [file, journal] of journals) for (const names of journal.batches.values()) {
        for (const name of Object.values(names)) renamedExports.add(`${file}\0${name}`);
      }
      const chunkBindings = new Map(), owners = new Map(), plans = new Map();
      for (const [id, state] of completed) {
        const analysis = analyzeChunk(state.sources, { selected: state.selected, ignored: state.ignored, chunkSiblings: state.chunkSiblings });
        for (const name of Object.values(state.journal?.batches.get('program-resolved') ?? {})) {
          const record = analysis.bindings.get(name);
          if (record) renamedExports.add(`${record.file}\0${name}`);
        }
        const bindings = new Map([...analysis.bindings].filter(([, record]) => record.declarations.length === 1)
          .map(([name, record]) => [name, { file: record.file, key: record.key }]));
        for (const file of state.chunk.files) { chunkBindings.set(file, bindings); owners.set(file, id); }
      }
      return { chunkBindings, renamedExports,
        resolve(file, proposals) {
          const id = owners.get(file), state = completed.get(id);
          if (state.readOnly) return new Map();
          const analysis = analyzeChunk(state.sources);
          const byKey = new Map([...analysis.bindings].flatMap(([name, record]) => record.declarations
            .filter(declaration => declaration.file === file).map(declaration => [declaration.binding.identifier.start, name])));
          if (!plans.has(id)) plans.set(id, new Map());
          const all = plans.get(id);
          for (const [key, name] of proposals) {
            if (!byKey.has(key)) throw new Error(`unknown chunk import key: ${file}:${key}`);
            all.set(byKey.get(key), name);
          }
          const resolved = resolveChunkNames(state.sources, all, { selected: state.selected, ignored: state.ignored, chunkSiblings: state.chunkSiblings, allowImports: true, restoreExternalImports: true });
          return new Map([...resolved].filter(([name]) => analysis.bindings.get(name).file === file).map(([name, value]) => [analysis.bindings.get(name).key, value]));
        },
        apply(sources, emit, summary) {
          for (const [id, state] of completed) {
            if (state.readOnly) continue;
            const proposals = plans.get(id) ?? new Map();
            const names = resolveChunkNames(state.sources, proposals, { selected: state.selected, ignored: state.ignored, chunkSiblings: state.chunkSiblings, allowImports: true, restoreExternalImports: true });
            const after = applyChunkRenames(state.sources, names);
            const reason = checkChunkRewrite(state.sources, after);
            if (reason) {
              summary.rejected += state.chunk.files.filter(file => selected.has(file)).length;
              if (state.pending) state.journal.complete(state.inputs, 'rejected', reason);
              for (const [file, source] of state.inputs) sources.set(file, source);
              emit({ chunk: id, status: 'imports-rejected', reason });
              continue;
            }
            if (state.pending || names.size) {
              state.journal.commit(after, { status: state.pending ? 'renamed' : 'imports' });
              if (state.pending) summary.completed += state.count;
              summary.chunkSiblingFiles = (summary.chunkSiblingFiles ?? 0) + [...after].filter(([file, source]) => !selected.has(file) && source !== state.inputs.get(file)).length;
              emit({ chunk: id, status: state.pending ? 'renamed' : 'imports', files: state.count, batches: state.batches });
            }
            state.pending = false;
            state.sources = after;
            for (const [file, source] of after) sources.set(file, source);
          }
        },
      };
    },
  };
  return driver;
}
