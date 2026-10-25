import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { runChunkWorkers } from '../scripts/chunk-workers.mjs';
import { loadChunks } from '../scripts/chunk-manifest.mjs';
import { openChunkJournal } from '../scripts/chunk-journal.mjs';
import { planChunk } from '../scripts/chunks.mjs';
import { syntaxFingerprint } from '../scripts/syntax/stage.mjs';
import { importMergeVersion } from '../scripts/merge-imports.mjs';
import { createNamer } from '../scripts/model.mjs';
import { modelService } from './helpers/model-service.mjs';

async function fixture(t, maxInputTokens) {
  const root = await mkdtemp(join(tmpdir(), 'chunk-budget-isolation-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const jobs = ['blocked', 'following'].map(id => ({ id, files: [`${id}-declare.js`, `${id}-use.js`] }));
  const inputs = new Map();
  for (const job of jobs) {
    const sources = new Map([[job.files[0], 'const occupiedValue = 1; const a = 2;\n'],
      [job.files[1], 'globalThis.result = a + occupiedValue;\n']]);
    inputs.set(job.id, sources);
    for (const [file, source] of sources) await writeFile(join(root, file), source);
  }
  const chunks = join(root, 'chunks.json');
  await writeFile(chunks, JSON.stringify({ version: 1, chunks: jobs }));
  const settings = { command: 'rename-bindings', chunks, chunkSiblings: true,
    minSymbolLength: 4, maxInputTokens, concurrency: 1 };
  const events = [], summary = { completed: 0, skipped: 0, rejected: 0, failedBatches: 0, exitCode: 0 };
  return { root, jobs, inputs, settings, selected: new Set(jobs.flatMap(job => job.files)), size: 1,
    withSlot: task => task(), emit: event => events.push(event), events, summary,
    sources: new Map(), phase: () => () => {} };
}

test('an irreducible collision budget failure leaves its chunk resumable and runs the next chunk', async t => {
  const options = await fixture(t, 1);
  const { fingerprint } = await loadChunks(options.settings.chunks, options.root);
  const journalOptions = { manifest: fingerprint, ignored: [], chunkSiblings: true,
    minSymbolLength: 4, maxInputTokens: 1, syntax: syntaxFingerprint, mergeImports: importMergeVersion };
  for (const job of options.jobs) {
    const inputs = options.inputs.get(job.id);
    const journal = openChunkJournal(options.root, job.id, inputs, journalOptions);
    journal.begin(job.files);
    for (const batch of planChunk(inputs, { ...options.settings, selected: options.selected, ignored: new Set() })) {
      journal.batch(batch.id, new Map(batch.keys.map(key => [key, job.id === 'blocked' ? 'occupiedValue' : 'readAmount'])));
    }
  }
  const results = await runChunkWorkers(options.jobs, options);
  assert.deepEqual(results.map(result => result.type), ['failed', 'done']);
  assert.equal(options.summary.exitCode, 1);
  assert.equal(options.summary.completed, 2);
  const failed = options.events.find(event => event.chunk === 'blocked' && event.status === 'failed');
  assert.match(failed.reason, /Collision batch collision:program:a cannot fit the input token budget \(1\)/);
  for (const [file, source] of options.inputs.get('blocked')) assert.equal(await readFile(join(options.root, file), 'utf8'), source);
  assert.equal(openChunkJournal(options.root, 'blocked', options.inputs.get('blocked'), journalOptions).done, null);
  const code = (await Promise.all(options.jobs[1].files.map(file => readFile(join(options.root, file), 'utf8')))).join('\n');
  assert.match(code, /const readAmount = 2/);
  assert.equal(Function(`${code}\nreturn globalThis.result;`)(), 3);
});

for (const code of ['AUTH', 'USAGE_LIMIT']) test(`${code} still stops queued chunks through the local model transport`, async t => {
  const options = await fixture(t, 100000);
  let requests = 0;
  const service = await modelService(t, options.root, async (req, res) => {
    for await (const part of req) { /* Drain the real client's request body. */ }
    requests++;
    res.writeHead(code === 'AUTH' ? 401 : 429);
    res.end(JSON.stringify({ error: { type: code === 'AUTH' ? 'invalid_grant' : 'usage_limit_reached' } }));
  });
  const namer = createNamer({ ...service, refreshUrl: `${service.baseUrl}/oauth/token`, retryDelayMs: 1 });
  t.after(() => namer.close());
  await assert.rejects(runChunkWorkers(options.jobs, { ...options, namer }), { code });
  assert(requests > 0);
  assert.equal(options.events.filter(event => event.status === 'planned').length, 1);
  assert.equal(options.summary.completed, 0);
  for (const [file, source] of options.inputs.get('following')) assert.equal(await readFile(join(options.root, file), 'utf8'), source);
});
