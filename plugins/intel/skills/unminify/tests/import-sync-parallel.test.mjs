import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { registerHooks } from 'node:module';
import * as select from '../scripts/select.mjs';
import * as syntax from '../scripts/syntax/stage.mjs';
import * as rename from '../scripts/rename.mjs';
import * as checks from '../scripts/checks.mjs';
import { writeImportSyncFixture, importSyncSnapshot } from './helpers/import-sync-fixture.mjs';

// Observe actual whole-chunk Babel graphs at their cache insertion boundary.
const observations = { active: false, references: [], peak: 0 };
globalThis.__unminifyImportAnalysis = analysis => {
  if (!observations.active) return;
  assert([...analysis.files.values()].every(file => file.ast?.type === 'File'));
  observations.references.push(new WeakRef(analysis));
  observations.peak = Math.max(observations.peak, observations.references.filter(ref => ref.deref()).length);
};
registerHooks({ load(url, context, next) {
  const result = next(url, context);
  if (!url.endsWith('/chunks.mjs')) return result;
  const source = String(result.source), needle = '  inspections.set(sources, {';
  assert(source.includes(needle));
  return { ...result, source: source.replace(needle, `  globalThis.__unminifyImportAnalysis(analysis);\n${needle}`) };
} });

const { run } = await import('../scripts/cli.mjs');

async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), 'unminify-import-sync-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  await writeImportSyncFixture(root);
  const workers = [];
  const dependencies = { ...select, ...syntax, ...rename, ...checks,
    syntaxPool: { size: 3, onWorker: event => workers.push(event) },
    createNamer() { throw new Error('sync-imports must never create a model client'); },
    output() {},
  };
  const options = { paths: [root], chunks: join(root, 'chunks.json'), minSymbolLength: 4 };
  return { root, workers, run: (command, output) => run({ ...options, command }, { ...dependencies, ...(output ? { output } : {}) }) };
}

async function baseline() {
  return JSON.parse(await readFile(new URL('./fixtures/import-sync-baseline.json', import.meta.url), 'utf8'));
}

test('sync-imports uses several real workers and preserves captured 0.71.4 source and journal bytes', async t => {
  const f = await fixture(t);
  assert.equal((await f.run('restore-syntax')).exitCode, 0);
  f.workers.length = 0;
  observations.active = true; observations.references.length = 0; observations.peak = 0;
  try { assert.equal((await f.run('sync-imports')).exitCode, 0); }
  finally { observations.active = false; }
  assert(observations.peak <= 1, `simultaneously retained ${observations.peak} real whole-chunk Babel analyses`);
  assert.deepEqual(await importSyncSnapshot(f.root), await baseline());
  const events = (await readFile(join(f.root, '.unminify/performance.jsonl'), 'utf8')).trim().split('\n').map(JSON.parse);
  const written = events.filter(event => event.type === 'file_end' && event.phase === 'followImports' && event.written_at !== null);
  assert(written.length > 1);
  for (const event of written) for (const step of ['resolveNames', 'applyRenames', 'checkRewrite']) {
    assert(event.cpu_ms[step] > 0, `${event.file} must retain ${step} worker timings`);
  }
  for (const kind of ['import-analyze', 'import-apply', 'import-check', 'import-chunk-analyze']) {
    const ids = new Set(f.workers.filter(event => event.kind === kind).map(event => event.threadId));
    assert(ids.size > 1, `${kind} must run on multiple actual workers, saw ${ids.size}`);
  }
});

test('sync-imports resumes after a committed chunk before ordinary writes byte-identically', async t => {
  const f = await fixture(t);
  assert.equal((await f.run('restore-syntax')).exitCode, 0);
  await assert.rejects(f.run('sync-imports', line => {
    const event = JSON.parse(line);
    if (event.chunk && event.status === 'imports') throw new Error('authored interruption after chunk commit');
  }), /authored interruption/);
  assert.equal((await f.run('sync-imports')).exitCode, 0);
  assert.deepEqual(await importSyncSnapshot(f.root), await baseline());
  const once = await importSyncSnapshot(f.root);
  assert.equal((await f.run('sync-imports')).exitCode, 0);
  assert.deepEqual(await importSyncSnapshot(f.root), once);
});

test('sync-imports recovers a real SIGKILL between chunk member replacements', async t => {
  const { execFile } = await import('node:child_process');
  const { promisify } = await import('node:util');
  const { fileURLToPath } = await import('node:url');
  const f = await fixture(t);
  assert.equal((await f.run('restore-syntax')).exitCode, 0);
  await assert.rejects(promisify(execFile)(process.execPath, [
    '--import', fileURLToPath(new URL('./helpers/import-sync-interrupt.mjs', import.meta.url)),
    fileURLToPath(new URL('../scripts/cli.mjs', import.meta.url)),
    'sync-imports', f.root, '--chunks', join(f.root, 'chunks.json'), '--min-symbol-length', '4',
  ], { timeout: 30_000 }), error => error.signal === 'SIGKILL');
  assert.match(await readFile(join(f.root, 'first.mjs'), 'utf8'), /import \{ readFile \}/);
  assert.match(await readFile(join(f.root, 'middle.mjs'), 'utf8'), /h\(a\(input\)\)/);
  assert.equal((await f.run('sync-imports')).exitCode, 0);
  assert.deepEqual(await importSyncSnapshot(f.root), await baseline());
});
