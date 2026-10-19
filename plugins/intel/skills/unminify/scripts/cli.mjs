#!/usr/bin/env node
import { execFileSync } from 'node:child_process';
import { readFileSync, existsSync, realpathSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { appendRecord, atomicWrite, openJournal } from './journal.mjs';

const defaults = { concurrency: 20, maxIds: 60, contextBytes: 8000, shortLen: 3 };
const usageKeys = ['input_tokens', 'cached_input_tokens', 'output_tokens', 'reasoning_output_tokens'];
const help = 'unminify <dir> [--ignore <file>] [--concurrency 20] [--max-ids 60] [--context-bytes 8000] [--short-len 3]';

export function parseArgs(args) {
  const options = { ...defaults };
  const flags = { '--ignore': 'ignoreFile', '--concurrency': 'concurrency', '--max-ids': 'maxIds', '--context-bytes': 'contextBytes', '--short-len': 'shortLen' };
  for (let index = 0; index < args.length; index++) {
    const arg = args[index];
    if (arg === '--help' || arg === '-h') return { help: true };
    if (arg.startsWith('-')) {
      const key = flags[arg], value = args[++index];
      if (!key || !value || value.startsWith('--')) throw new Error(`invalid option: ${arg}\n${help}`);
      if (key === 'ignoreFile') options[key] = resolve(value);
      else {
        if (!/^\d+$/.test(value) || !Number.isSafeInteger(Number(value)) || Number(value) < 1) throw new Error(`${arg} must be a positive integer`);
        options[key] = Number(value);
      }
    } else if (options.dir) throw new Error(`unexpected argument: ${arg}`);
    else options.dir = resolve(arg);
  }
  if (!options.dir) throw new Error(help);
  return options;
}

async function modules() {
  const [select, rename, checks, model] = await Promise.all([
    import('./select.mjs'), import('./rename.mjs'), import('./checks.mjs'), import('./model.mjs'),
  ]);
  return { ...select, ...rename, ...checks, ...model };
}

// Callers may supply the documented module contracts to run without a model.
export async function run(options, dependencies) {
  const settings = { ...defaults, ...options };
  for (const key of Object.keys(defaults)) {
    if (!Number.isSafeInteger(settings[key]) || settings[key] < 1) throw new Error(`${key} must be a positive integer`);
  }
  const api = dependencies ?? await modules();
  const output = api.output ?? (line => console.log(line));
  const emit = record => output(JSON.stringify(record));
  const dir = resolve(settings.dir);
  const root = execFileSync('git', ['rev-parse', '--show-toplevel'], { cwd: dir, encoding: 'utf8' }).trim();
  const ignoreFile = settings.ignoreFile ? resolve(settings.ignoreFile) : join(root, '.ignore');
  const files = await api.listFiles(dir, { ignoreFile: settings.ignoreFile || existsSync(ignoreFile) ? ignoreFile : undefined });
  const planOptions = { shortLen: settings.shortLen, maxIds: settings.maxIds, contextBytes: settings.contextBytes };
  const summary = { type: 'summary', files: files.length, completed: 0, skipped: 0, rejected: 0, failedBatches: 0, requests: 0, tokens: Object.fromEntries(usageKeys.map(key => [key, 0])), quota: {}, exitCode: 0 };
  let stop = null, active = 0;
  const waiters = [];
  const sources = new Map(), journals = new Map();
  const log = record => {
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
  const namer = await api.createNamer({ concurrency: settings.concurrency, codexHome: settings.codexHome, log });
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
    const renames = new Map();
    const { stages } = api.planFile(source, planOptions);
    for (const stage of stages) {
      const results = await Promise.allSettled(stage.map(async batch => {
        if (journal.batches.has(batch.id)) {
          for (const [key, name] of Object.entries(journal.batches.get(batch.id))) renames.set(Number(key), name);
          return;
        }
        if (stop) throw stop;
        await withSlot(async () => {
          const payload = api.renderBatch(source, batch, renames);
          let response;
          try { response = await namer.name(payload); } catch (error) {
            if (error.code === 'USAGE_LIMIT' || error.code === 'AUTH') throw error;
            journal.batch(batch.id, new Map(), error.message);
            summary.failedBatches++;
            emit({ file, status: 'batch-failed', batch: batch.id, reason: error.message });
            return;
          }
          const entries = Object.entries(payload.placeholders);
          if (!response.names || entries.some(([placeholder]) => typeof response.names[placeholder] !== 'string') || Object.keys(response.names).length !== entries.length) throw new Error(`invalid names for ${file}:${batch.id}`);
          const proposals = new Map(renames);
          for (const [placeholder, key] of entries) proposals.set(key, response.names[placeholder]);
          const resolved = api.resolveNames(source, proposals);
          const names = new Map(batch.keys.filter(key => resolved.has(key)).map(key => [key, resolved.get(key)]));
          // Persist before exposing a reply to any later render or starting queued work.
          journal.batch(batch.id, names);
          for (const [key, name] of names) renames.set(key, name);
        });
      }));
      const failure = results.find(result => result.status === 'rejected');
      if (failure) throw failure.reason;
    }
    const after = api.applyRenames(source, renames);
    const reason = await api.checkRewrite(source, after);
    if (reason !== null) {
      journal.complete(source, 'rejected', reason);
      summary.rejected++;
      emit({ file, status: 'rejected', reason });
      return;
    }
    journal.prepare(source, after, 'renamed');
    atomicWrite(path, after);
    journal.complete(after, 'renamed');
    sources.set(file, after);
    summary.completed++;
    emit({ file, status: 'renamed', batches: stages.flat().length });
  }
  try {
    const results = await Promise.allSettled(files.map(processFile));
    for (let index = 0; index < results.length; index++) {
      if (results[index].status !== 'rejected') continue;
      summary.exitCode = 1;
      emit({ file: files[index], status: 'interrupted', reason: results[index].reason.message });
    }
    if (!summary.exitCode) {
      const changes = await api.followImports(sources);
      for (const [file, after] of changes) {
        if (!sources.has(file)) throw new Error(`followImports returned an unknown file: ${file}`);
        const before = sources.get(file), journal = journals.get(file);
        const reason = await api.checkRewrite(before, after);
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
        emit({ file, status: 'imports' });
      }
    }
  } finally { await namer.close(); }
  if (stop) {
    output(`${stop.code}: ${stop.code === 'AUTH' ? 'Run codex login, then resume.' : 'Wait for quota to reset, then resume.'} Resume with the same unminify command; completed replies are saved.`);
    summary.exitCode = 1;
  }
  if (summary.rejected || summary.failedBatches) summary.exitCode = 1;
  summary.tokens.total_tokens = summary.tokens.input_tokens + summary.tokens.output_tokens;
  if (!Object.keys(summary.quota).length) summary.quota = "没测到";
  emit(summary);
  return summary;
}

if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
  try {
    const options = parseArgs(process.argv.slice(2));
    if (options.help) console.log(help);
    else process.exitCode = (await run(options)).exitCode;
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
