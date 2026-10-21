#!/usr/bin/env node
import { loadHistory, loadImportContext } from './history.mjs';
import { createChunkIgnore } from './chunk-manifest.mjs';
import { runMergeFile } from './merge-imports.mjs';
import { loadFragmentContext } from './fragments.mjs';
import { createSyntaxPool } from './syntax/pool.mjs';
import { runSyntaxFileParallel } from './syntax/parallel-stage.mjs';
import { randomUUID } from 'node:crypto';
import { reconsiderNames } from './collision.mjs';
import { packBatches } from './pack.mjs';
import { withTimings, timed, intervalMs, requestStats } from './timing.mjs';
import { execFileSync } from 'node:child_process';
import { readFileSync, existsSync, realpathSync, statSync } from 'node:fs';
import { resolve, join, dirname } from 'node:path';
import { pathToFileURL } from 'node:url';
import { appendRecord, atomicWrite, openJournal } from './journal.mjs';

const defaults = { concurrency: 20, maxInputTokens: 100000, minSymbolLength: 4 };
const usageKeys = ['input_tokens', 'cached_input_tokens', 'output_tokens', 'reasoning_output_tokens'];
const commands = ['restore-syntax', 'rename-bindings', 'sync-imports'];
const help = [
  'unminify restore-syntax <path>... [--ignore <file>] [--chunks <file>]',
  'unminify rename-bindings <path>... [--ignore <file>] [--chunks <file>] [--min-symbol-length 4] [--concurrency 20] [--max-input-tokens 100000]',
  'unminify sync-imports <path>... [--ignore <file>] [--chunks <file>] [--min-symbol-length 4]',
].join('\n');

export function parseArgs(args) {
  if (args.length === 1 && ['--help', '-h'].includes(args[0])) return { help: true };
  const [command, ...rest] = args;
  if (!commands.includes(command)) throw new Error(help);
  args = rest;
  const options = { command, ...defaults };
  const flags = { '--chunks': 'chunks', '--ignore': 'ignoreFile', '--concurrency': 'concurrency', '--max-input-tokens': 'maxInputTokens', '--min-symbol-length': 'minSymbolLength' };
  for (let index = 0; index < args.length; index++) {
    const arg = args[index];
    if (arg === '--help' || arg === '-h') return { help: true };
    if (arg.startsWith('-')) {
      const key = flags[arg], value = args[++index];
      if (!key || command === 'restore-syntax' && !['chunks', 'ignoreFile'].includes(key)
        || command === 'sync-imports' && ['concurrency', 'maxInputTokens'].includes(key)
        || !value || value.startsWith('--')) throw new Error(`invalid option: ${arg}\n${help}`);
      if (key === 'ignoreFile' || key === 'chunks') options[key] = resolve(value);
      else {
        if (!/^\d+$/.test(value) || !Number.isSafeInteger(Number(value)) || Number(value) < 1) throw new Error(`${arg} must be a positive integer`);
        options[key] = Number(value);
      }
    } else (options.paths ??= []).push(resolve(arg));
  }
  if (!options.paths) throw new Error(help);
  return options;
}

async function modules() {
  const [select, rename, checks, model, syntax] = await Promise.all([
    import('./select.mjs'), import('./rename.mjs'), import('./checks.mjs'), import('./model.mjs'), import('./syntax/stage.mjs'),
  ]);
  return { ...select, ...rename, ...checks, ...model, ...syntax };
}

// Callers may supply the documented module contracts to run without a model.
export async function run(options, dependencies) {
  // Chunk runs always rewrite references in unselected sibling fragments.
  const settings = { ...defaults, ...options, command: options.command ?? 'all', chunkSiblings: !!options.chunks };
  if (settings.command !== 'all' && !commands.includes(settings.command)) throw new Error(help);
  const restoring = settings.command === 'restore-syntax' || settings.command === 'all';
  const naming = settings.command === 'rename-bindings' || settings.command === 'all';
  const syncing = settings.command === 'sync-imports' || settings.command === 'all';
  for (const key of Object.keys(defaults)) {
    if (!Number.isSafeInteger(settings[key]) || settings[key] < 1) throw new Error(`${key} must be a positive integer`);
  }
  const startedAt = Date.now(), runId = randomUUID();
  const paths = settings.paths.map(target => resolve(target));
  const firstDir = statSync(paths[0]).isDirectory() ? paths[0] : dirname(paths[0]);
  const root = execFileSync('git', ['rev-parse', '--show-toplevel'], { cwd: firstDir, encoding: 'utf8' }).trim();
  const performancePath = join(root, '.unminify', 'performance.jsonl');
  const event = record => appendRecord(performancePath, { ...record, run_id: runId });
  event({ type: 'run_start', at: startedAt });
  const phases = {}, requests = [], pauses = [], fileMetrics = new Map();
  function phase(name, file) {
    const start = Date.now();
    event({ type: 'phase_start', phase: name, file, at: start });
    return () => {
      const end = Date.now();
      (phases[name] ??= []).push([start, end]);
      event({ type: 'phase_end', phase: name, file, at: end, started_at: start });
    };
  }
  let selectionEnd = phase('selection_plan');
  const output = dependencies?.output ?? (line => console.log(line));
  const emit = record => output(JSON.stringify(record));
  const summary = { type: 'summary', run_id: runId, files: 0, completed: 0, skipped: 0, rejected: 0,
    failedBatches: 0, requests: 0, tokens: Object.fromEntries(usageKeys.map(key => [key, 0])), quota: {}, exitCode: 0 };
  function fileMetricsFor(file, at = Date.now()) {
    if (!fileMetrics.has(file)) {
      const metrics = { file, started_at: at, last_batch_at: null, written_at: null,
        cpu_ms: Object.fromEntries(['parse', 'planFile', 'renderBatch', 'resolveNames', 'applyRenames', 'checkRewrite'].map(key => [key, 0])) };
      fileMetrics.set(file, metrics);
    }
    return fileMetrics.get(file);
  }
  let api, namer, syntaxPool;
  try {
    api = dependencies ?? await modules();
    syntaxPool = createSyntaxPool(dependencies?.syntaxPool);
    const ignores = await createChunkIgnore(root, settings.ignoreFile);
    const chunkDriver = await (await import('./chunk-run.mjs')).createChunkRun(root, settings, ignores);
    // Overlapping paths list a file once.
    const listed = new Set();
    for (const target of paths) {
      for (const file of await api.listFiles(target, {
        root, ignores, pool: syntaxPool,
        onParse(file, at, elapsed) { fileMetricsFor(file, at).cpu_ms.parse += elapsed; },
      })) listed.add(file);
    }
    const selectedFiles = [...listed];
    chunkDriver.select(selectedFiles, new Set());
    const siblings = chunkDriver.jobs.flatMap(chunk => chunk.files).filter(file => !listed.has(file));
    for (const file of await api.listFiles(siblings.map(file => join(root, file)), { root, ignores, pool: syntaxPool })) listed.add(file);
    const files = [...listed].sort();
    const fragmentContext = await loadFragmentContext(root, files, syntaxPool);
    const { fragments } = fragmentContext;
    chunkDriver.select(selectedFiles, fragments);
    const planOptions = { minSymbolLength: settings.minSymbolLength, maxInputTokens: settings.maxInputTokens };
    summary.files = selectedFiles.length;
    selectionEnd(); selectionEnd = null;
    let stop = null, active = 0;
    const waiters = [];
    const sources = new Map(), journals = new Map();
    const log = record => {
      record = { ...record, run_id: runId };
      requests.push(record);
      appendRecord(join(root, '.unminify', 'requests.jsonl'), record);
      if (record.kind !== 'model') return;
      summary.requests++;
      for (const key of usageKeys) summary.tokens[key] += record.usage?.[key] ?? 0;
      for (const [header, value] of Object.entries(record.rate_headers ?? {})) {
        if (!header.endsWith('-used-percent') || !Number.isFinite(Number(value))) continue;
        const percent = Number(value);
        const entry = summary.quota[header] ?? { before: percent };
        entry.after = percent;
        entry.change = percent - entry.before;
        summary.quota[header] = entry;
      }
    };
    // Finish every syntax write before creating any rename plan or request.
    async function syntaxFiles(task) {
      const results = await Promise.allSettled(files.filter(file => !chunkDriver.files.has(file)).map(task));
      const failure = results.find(result => result.status === 'rejected');
      if (failure) throw failure.reason;
      return results.map(result => result.value);
    }
    if (restoring && api.runSyntaxFile) {
      const records = await syntaxFiles(async file => {
        // Merge checkpoints own the text after syntax, including the write/complete window.
        if (existsSync(join(root, '.unminify', 'merge-imports', `${file}.jsonl`))) {
          return { file, status: 'syntax-skipped' };
        }
        const endSyntax = phase('syntax', file);
        try { return await runSyntaxFileParallel(root, file, { fragment: fragments.has(file), fragmentContext }, syntaxPool); }
        finally { endSyntax(); }
      });
      records.forEach(emit);
    }
    if (restoring) {
      const records = await syntaxFiles(async file => {
        const endMerge = phase('merge_imports');
        try { return await runMergeFile(root, file, syntaxPool); } finally { endMerge(); }
      });
      records.forEach(emit);
      const rejected = await chunkDriver.restore({ api, emit, summary, sources, phase, fragmentContext, pool: syntaxPool });
      if (rejected || !naming) {
        summary.exitCode = summary.rejected ? 1 : 0;
        return summary;
      }
    }
    if (settings.command === 'rename-bindings') {
      const [chunkPending, ordinaryPending] = await Promise.all([
        chunkDriver.syntaxPending({ fragmentContext, pool: syntaxPool }),
        syntaxFiles(async file => {
          const source = readFileSync(join(root, file), 'utf8');
          const restored = await syntaxPool.run({ kind: 'transform', source,
            options: { fragment: fragments.has(file), fragmentContext } });
          const merged = await syntaxPool.run({ kind: 'merge', sources: new Map([[file, restored.source]]) });
          return source !== merged.get(file) ? file : null;
        }),
      ]);
      const pending = [...chunkPending, ...ordinaryPending.filter(Boolean)];
      if (pending.length) throw new Error(`Run restore-syntax before rename-bindings:\n${[...new Set(pending)].sort().join('\n')}`);
    }
    await syntaxPool.close(); syntaxPool = undefined;
    if (naming) namer = await api.createNamer({ concurrency: settings.concurrency, codexHome: settings.codexHome, log, runId, queued: () => waiters.length,
      event(record) {
        event(record);
        if (record.type === 'pause_end') pauses.push([record.started_at, record.ended_at]);
      } });
    async function withSlot(task) {
      if (active >= settings.concurrency) await new Promise(resolve => waiters.push(resolve));
      else active++;
      try {
        if (stop) throw stop;
        return await task();
      } catch (error) {
        if (error.code === 'USAGE_LIMIT' || error.code === 'AUTH') stop ??= error;
        throw error;
      } finally {
        const next = waiters.shift();
        if (next) next();
        else active--;
      }
    }
    async function processFile(file) {
      const metrics = fileMetricsFor(file);
      const fileOptions = { ...planOptions, fragment: fragments.has(file) };
      metrics.started_at = Date.now();
      event({ type: 'file_start', file, at: metrics.started_at });
      let stages;
      return withTimings(metrics.cpu_ms, async () => {
        try {
          const path = join(root, file);
          const source = readFileSync(path, 'utf8');
          sources.set(file, source);
          const journal = openJournal(root, file, source, planOptions);
          journals.set(file, journal);
          if (journal.done) {
            summary.skipped++;
            if (journal.done.status === 'rejected') summary.rejected++;
            emit({ file, status: 'skipped', previous: journal.done.status, ...(journal.done.reason ? { reason: journal.done.reason } : {}) });
            return;
          }
          if (stop) throw stop;
          const renames = new Map();
          let batchFailed = false;
          const endPlanning = phase('selection_plan', file);
          try { ({ stages } = timed('planFile', () => api.planFile(source, fileOptions))); }
          finally { endPlanning(); }
          const endModel = phase('model', file);
          try {
            for (const stage of stages) {
              // Replay the whole stage before rendering any pending request.
              for (const batch of stage) {
                if (!journal.batches.has(batch.id)) continue;
                for (const [key, name] of Object.entries(journal.batches.get(batch.id))) renames.set(Number(key), name);
              }
              if (stop) throw stop;
              const pending = stage.filter(batch => !journal.batches.has(batch.id));
              const packed = packBatches(pending.filter(batch => !journal.replies.has(`proposal:${batch.id}`)), (batch, placeholders) =>
                timed('renderBatch', () => api.renderBatch(source, batch, renames, placeholders)), settings);
              const results = await Promise.allSettled(packed.map(async ({ parts, payload }) => {
                try {
                  const queuedAt = Date.now();
                  await withSlot(async () => {
                    let response;
                    const batchIds = parts.map(({ batch }) => batch.id);
                    try { response = await namer.name(payload, { file, batch_id: batchIds.length === 1 ? batchIds[0] : null,
                      batch_ids: batchIds, name_count: Object.keys(payload.placeholders).length,
                      phase: 'model', queued_at: queuedAt }); } catch (error) {
                      if (error.code === 'USAGE_LIMIT' || error.code === 'AUTH') throw error;
                      // Unrecorded batches keep the file unfinished, so resume retries them.
                      batchFailed = true;
                      for (const { batch } of parts) {
                        summary.failedBatches++;
                        emit({ file, status: 'batch-failed', batch: batch.id, reason: error.message });
                      }
                      return;
                    }
                    const entries = Object.entries(payload.placeholders);
                    if (!response.names || entries.some(([placeholder]) => typeof response.names[placeholder] !== 'string') || Object.keys(response.names).length !== entries.length) throw new Error(`invalid names for ${file}:${batchIds.join(',')}`);
                    for (const { batch, payload: original } of parts) {
                      journal.response(`proposal:${batch.id}`, new Map(Object.entries(original.placeholders).map(([placeholder, key]) => [key, response.names[placeholder]])));
                    }
                  });
                } finally { metrics.last_batch_at = Date.now(); }
              }));
              const failure = results.find(result => result.status === 'rejected');
              // Accepted replies can resolve locally after quota stops further model requests.
              let resolutionFailure;
              for (const batch of pending) {
                try {
                  const raw = journal.replies.get(`proposal:${batch.id}`);
                  if (!raw) continue;
                  const proposals = new Map(renames);
                  for (const [key, name] of Object.entries(raw)) proposals.set(Number(key), name);
                  const resolved = await reconsiderNames({ proposals, journal, id: batch.id, namer: { name: (...args) => withSlot(() => namer.name(...args)) },
                    maxInputTokens: settings.maxInputTokens, telemetry: { file },
                    resolve: (values, onCollision) => timed('resolveNames', () => api.resolveNames(source, values, { ...fileOptions, onCollision })) });
                  const names = new Map(batch.keys.filter(key => resolved.has(key)).map(key => [key, resolved.get(key)]));
                  journal.batch(batch.id, names);
                  for (const [key, name] of names) renames.set(key, name);
                } catch (error) { resolutionFailure ??= error; }
              }
              if (failure) throw failure.reason;
              if (resolutionFailure) throw resolutionFailure;
            }
          } finally { endModel(); }
          if (batchFailed) return;
          const endWriting = phase('check_write', file);
          try {
            // Recheck replayed journal proposals against the current file policy.
            const resolved = timed('resolveNames', () => api.resolveNames(source, renames, fileOptions));
            const after = timed('applyRenames', () => api.applyRenames(source, resolved));
            const reason = await timed('checkRewrite', () => api.checkRewrite(source, after, fileOptions));
            if (reason !== null) {
              journal.complete(source, 'rejected', reason);
              summary.rejected++;
              emit({ file, status: 'rejected', reason });
              return;
            }
            journal.prepare(source, after, 'renamed');
            atomicWrite(path, after);
            journal.complete(after, 'renamed');
            metrics.written_at = Date.now();
            sources.set(file, after);
            summary.completed++;
            emit({ file, status: 'renamed', batches: stages.flat().length });
          } finally { endWriting(); }
        } finally {
          if (stages) api.releasePlan?.(stages);
          stages = undefined;
          event({ type: 'file_end', phase: 'main', ...metrics, at: Date.now() });
        }
      });
    }
    try {
      // Keep a second window ready while request slots drain or files write back.
      let tasks = naming ? [...chunkDriver.jobs, ...files.filter(file => !chunkDriver.files.has(file))] : [];
      do {
        const fileConcurrency = Math.min(tasks.length, settings.concurrency * 2);
        let nextFile = 0;
        async function worker() {
          while (nextFile < tasks.length) {
            const task = tasks[nextFile++], file = typeof task === 'string' ? task : task.id;
            try {
              if (typeof task === 'string') await processFile(task);
              else await chunkDriver.process(task, { api, namer, withSlot, emit, summary, sources, phase, fragmentContext });
            }
            catch (error) {
              summary.exitCode = 1;
              emit({ file, status: 'interrupted', reason: error.message });
            }
          }
        }
        await Promise.all(Array.from({ length: fileConcurrency }, worker));
        if (!syncing) break;
        if (!naming) await chunkDriver.prepareImports(sources);
        loadHistory(root, { sources, journals, chunks: chunkDriver, ignores, emit });
        if (!naming) for (const file of files) {
          if (chunkDriver.files.has(file) || journals.has(file)) continue;
          const source = readFileSync(join(root, file), 'utf8');
          sources.set(file, source);
          // Pending naming journals keep ownership of their offsets and source text.
          if (existsSync(join(root, '.unminify', `${file}.jsonl`))) continue;
          const journal = openJournal(root, join('imports', file), source, { minSymbolLength: settings.minSymbolLength });
          if (!journal.done) journal.complete(source, 'imports');
          journals.set(file, journal);
        }
        loadImportContext(root, sources);
        const endImports = phase('followImports');
        let changes;
        const chunkImports = chunkDriver.importOptions(journals);
        chunkImports.readOnly = new Set([...sources.keys()].filter(file => !journals.get(file)?.done && !chunkImports.chunkBindings.has(file)));
        try {
          changes = await api.followImports(sources, (file, task) => {
            const end = phase('followImports', file);
            try { return fileMetrics.has(file) ? withTimings(fileMetrics.get(file).cpu_ms, task) : task(); }
            finally { end(); }
          }, fragments, chunkImports, settings);
        }
        finally {
          endImports();
          for (const metrics of fileMetrics.values()) {
            if (!changes?.has(metrics.file) || !journals.get(metrics.file)?.done) event({ type: 'file_end', phase: 'followImports', ...metrics, written_at: null, at: Date.now() });
          }
        }
        chunkImports?.apply(sources, emit, summary);
        for (const [file, after] of changes) {
          if (!sources.has(file)) throw new Error(`followImports returned an unknown file: ${file}`);
          // Unfinished journals retain batch positions and the original source fingerprint.
          if (!journals.get(file)?.done) continue;
          const metrics = fileMetricsFor(file);
          let writtenAt = null;
          const endWriting = phase('check_write', file);
          try {
            const before = sources.get(file), journal = journals.get(file);
            const reason = await withTimings(metrics.cpu_ms, () => timed('checkRewrite', () => api.checkRewrite(before, after, { fragment: fragments.has(file) })));
            if (reason !== null) {
              journal.rejectImports(reason);
              summary.rejected++;
              emit({ file, status: 'imports-rejected', reason });
              continue;
            }
            journal.prepare(before, after, 'imports');
            atomicWrite(join(root, file), after);
            journal.complete(after, 'imports');
            sources.set(file, after);
            writtenAt = Date.now();
            emit({ file, status: 'imports' });
          } finally {
            endWriting();
            event({ type: 'file_end', phase: 'followImports', ...metrics, written_at: writtenAt, at: Date.now() });
          }
        }
        tasks = naming && !stop ? chunkDriver.more() : [];
      } while (tasks.length);
    } finally { await namer?.close(); namer = undefined; }
    if (stop) {
      output(`${stop.code}: ${stop.code === 'AUTH' ? 'Run codex login, then resume.' : 'Wait for quota to reset, then resume.'} Resume with the same unminify command; completed replies are saved.`);
      summary.exitCode = 1;
    }
    if (summary.rejected || summary.failedBatches) summary.exitCode = 1;
    return summary;
  } catch (error) {
    summary.exitCode = 1;
    throw error;
  } finally {
    try { await syntaxPool?.close(); await namer?.close(); }
    finally {
      selectionEnd?.();
      const endedAt = Date.now();
      summary.tokens.total_tokens = summary.tokens.input_tokens + summary.tokens.output_tokens;
      if (!Object.keys(summary.quota).length) summary.quota = "没测到";
      summary.total_ms = endedAt - startedAt;
      summary.phase_ms = Object.fromEntries(['selection_plan', ...(phases.syntax ? ['syntax'] : []), ...(phases.merge_imports ? ['merge_imports'] : []), 'model', 'followImports', 'check_write']
        .map(key => [key, intervalMs(phases[key] ?? [])]));
      Object.assign(summary, requestStats(requests, pauses, summary.total_ms));
      event({ type: 'run_end', at: endedAt, total_ms: summary.total_ms });
      event({ ...summary, at: endedAt });
      emit(summary);
    }
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
  try {
    const options = parseArgs(process.argv.slice(2));
    if (options.help) console.log(help);
    else process.exitCode = (await run(options)).exitCode;
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
