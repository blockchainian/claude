import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { createChunkRun } from '../scripts/chunk-run.mjs';
import * as select from '../scripts/select.mjs';
import * as rename from '../scripts/rename.mjs';
import * as checks from '../scripts/checks.mjs';
import { hash } from '../scripts/journal.mjs';

// Authored independent functions exercise the actual planner, scope checks and transaction.
async function fixture(t, count = 4) {
  const root = await mkdtemp(join(tmpdir(), 'chunk-local-concurrency-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const inputs = new Map(Array.from({ length: count }, (_, index) => [
    `member-${index}.js`, `function calculate${index}(a) { return a + ${index}; }\n`,
  ]));
  for (const [file, source] of inputs) await writeFile(join(root, file), source);
  const chunks = join(root, 'chunks.json');
  await writeFile(chunks, JSON.stringify({ version: 1, chunks: [{ id: 'authored', files: [...inputs.keys()] }] }));
  const settings = { chunks, concurrency: 2, minSymbolLength: 4, maxInputTokens: 100000, command: 'rename-bindings' };
  const journalPath = join(root, '.unminify/chunks', `${hash('authored')}.jsonl`);
  return { root, inputs, settings, journalPath,
    records: async () => (await readFile(journalPath, 'utf8')).trim().split('\n').map(line => JSON.parse(line)),
    outputs: async () => Promise.all([...inputs.keys()].map(file => readFile(join(root, file), 'utf8'))),
  };
}

function slots(limit) {
  let active = 0, stopped;
  const queue = [];
  return async task => {
    while (active >= limit) await new Promise(resolve => queue.push(resolve));
    if (stopped) throw stopped;
    active++;
    try { return await task(); }
    catch (error) {
      if (['AUTH', 'USAGE_LIMIT'].includes(error.code)) stopped = error;
      throw error;
    } finally { active--; queue.shift()?.(); }
  };
}

async function run(f, name, { withSlot = slots(f.settings.concurrency), api = {} } = {}) {
  const driver = await createChunkRun(f.root, f.settings, () => false);
  driver.select([...f.inputs.keys()], new Set());
  const summary = { completed: 0, skipped: 0, rejected: 0, failedBatches: 0 };
  const events = [];
  await driver.process(driver.jobs[0], { api: { ...select, ...rename, ...checks, ...api },
    namer: { name }, withSlot, summary, sources: new Map(), emit: event => events.push(event), phase: () => () => {} });
  return { summary, events };
}

const names = payload => ({ names: Object.fromEntries(Object.keys(payload.placeholders).map(key => [key, 'inputValue'])) });

test('local members overlap while sharing the request slot and journal in member order', async t => {
  const f = await fixture(t);
  let active = 0, maximum = 0;
  const requests = [];
  const { summary } = await run(f, async (payload, metadata) => {
    requests.push(metadata);
    maximum = Math.max(maximum, ++active);
    await delay(metadata.file === 'member-0.js' ? 60 : 10);
    active--;
    return names(payload);
  });
  assert.equal(maximum, 2, 'independent local members must fill the two shared slots');
  assert.equal(requests.length, 4);
  assert(requests.every(request => request.batch_ids.every(id => id.startsWith(`local:${request.file}:`))));
  assert.equal(summary.completed, 4);
  const records = await f.records();
  assert.deepEqual(records.filter(record => record.type === 'chunk-batch' && record.id.startsWith('local:'))
    .map(record => record.id.split(':')[1]), [...f.inputs.keys()]);
  assert.deepEqual(await f.outputs(), [...f.inputs.values()].map(source => source.replaceAll(/\ba\b/g, 'inputValue')));
});

test('arrival order cannot change output or complete journal bytes', async t => {
  const first = await fixture(t), second = await fixture(t);
  for (const [index, f] of [first, second].entries()) {
    await run(f, async (payload, metadata) => {
      const member = Number(/member-(\d+)/.exec(metadata.file)[1]);
      await delay(index ? 5 + member * 10 : 40 - member * 10);
      return names(payload);
    });
  }
  assert.deepEqual(await first.outputs(), await second.outputs());
  assert.equal(await readFile(first.journalPath, 'utf8'), await readFile(second.journalPath, 'utf8'));
});

test('legacy local batch records are reused without sending their requests again', async t => {
  const f = await fixture(t);
  await run(f, async payload => names(payload));
  const expected = await f.outputs();
  const records = await f.records();
  const prepare = records.findIndex(record => record.type === 'chunk-prepare');
  assert(prepare > 0);
  assert.equal(records.slice(0, prepare).filter(record => record.type === 'chunk-batch' && record.id.startsWith('local:')).length, 4);
  // An interrupted pre-transaction journal has the same legacy records as a serial run.
  await writeFile(f.journalPath, records.slice(0, prepare).map(record => JSON.stringify(record)).join('\n') + '\n');
  for (const [file, source] of f.inputs) await writeFile(join(f.root, file), source);
  let requests = 0;
  const { summary } = await run(f, async payload => { requests++; return names(payload); });
  assert.equal(requests, 0);
  assert.equal(summary.completed, 4);
  assert.deepEqual(await f.outputs(), expected);
});

for (const code of ['AUTH', 'USAGE_LIMIT']) test(`${code} stops queued local requests and drains accepted members before throwing`, async t => {
  const f = await fixture(t, 5);
  const started = [], finished = [];
  let active = 0;
  const failure = Object.assign(new Error(code), { code });
  await assert.rejects(run(f, async (payload, metadata) => {
    started.push(metadata.file);
    active++;
    try {
      await delay(metadata.file === 'member-0.js' ? 5 : 40);
      if (metadata.file === 'member-0.js') throw failure;
      return names(payload);
    } finally { active--; finished.push(metadata.file); }
  }), error => error === failure);
  assert.equal(active, 0, 'the rejected process must drain in-flight model calls');
  assert.deepEqual(started, ['member-0.js', 'member-1.js']);
  assert.deepEqual(finished.sort(), started);
  assert.deepEqual(await f.outputs(), [...f.inputs.values()]);
  const records = await f.records();
  assert(!records.some(record => ['chunk-prepare', 'chunk-done'].includes(record.type)));
  assert(records.some(record => record.type === 'chunk-batch' && record.id.startsWith('local:member-1.js:')),
    'an accepted peer reply remains available on resume');
});

test('a rejected member drains peers and rejects the whole transaction without writing sources', async t => {
  const f = await fixture(t);
  let active = 0;
  const { summary, events } = await run(f, async (payload, metadata) => {
    active++;
    await delay(metadata.file === 'member-0.js' ? 5 : 35);
    active--;
    return names(payload);
  }, { api: {
    applyRenames(source, proposed) {
      const rewritten = rename.applyRenames(source, proposed);
      // A malformed transform must be caught by the production scope/text validator.
      return rewritten.replace(' + 0;', ' + 99;');
    },
  } });
  assert.equal(active, 0);
  assert.equal(summary.completed, 0);
  assert.equal(summary.rejected, 4);
  assert(events.some(event => event.status === 'rejected' && event.reason === 'STRUCTURE_OR_TEXT_CHANGED'));
  assert.deepEqual(await f.outputs(), [...f.inputs.values()]);
  const records = await f.records();
  assert(!records.some(record => record.type === 'chunk-prepare'));
  assert.equal(records.at(-1).type, 'chunk-done');
  assert.equal(records.at(-1).status, 'rejected');
});

test('asynchronous rewrite checks yield to the event loop and preserve rejection reasons', async () => {
  assert.equal(typeof checks.checkRewriteAsync, 'function');
  const before = 'function calculate(a) { return a + 1; }\n';
  const after = 'function calculate(inputValue) { return inputValue + 1; }\n';
  let ticked = false;
  const tick = delay(0).then(() => { ticked = true; });
  assert.equal(await checks.checkRewriteAsync(before, after, { fragment: true }), null);
  assert(ticked, 'node --check must run without blocking the event loop');
  await tick;
  for (const invalid of [after.replace('+ 1', '+ 2'), 'function broken( {']) {
    assert.equal(await checks.checkRewriteAsync(before, invalid, { fragment: true }), checks.checkRewrite(before, invalid, { fragment: true }));
  }
});
