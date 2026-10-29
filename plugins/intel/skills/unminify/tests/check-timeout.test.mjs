import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { checkRewrite, checkRewriteAsync } from '../scripts/checks.mjs';
import { createChunkRun } from '../scripts/chunk-run.mjs';
import * as select from '../scripts/select.mjs';
import * as rename from '../scripts/rename.mjs';
import * as checks from '../scripts/checks.mjs';
import { hash } from '../scripts/journal.mjs';

test('a busy caller cannot time out a valid module syntax check', async () => {
  const before = 'export function calculate(a) { return a + 1; }\n';
  const after = 'export function calculate(inputValue) { return inputValue + 1; }\n';
  const result = checkRewriteAsync(before, after);
  // Real CPU work in another chunk can keep the caller busy beyond the child deadline.
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 11000);
  assert.equal(await result, null);
});

test('both syntax guards retain parser diagnostics without changing rejection codes', async () => {
  for (const check of [checkRewrite, checkRewriteAsync]) {
    for (const after of ['function broken( {', 'function calculate(a, a) { return a; }']) {
      const errors = [];
      assert.equal(await check('function calculate(a) { return a; }', after, {
        onParseError: message => errors.push(message),
      }), 'PARSE_FAIL');
      assert.equal(errors.length, 1);
      assert.match(errors[0], /Unexpected token|Argument name clash|Duplicate parameter/);
    }
  }
});

test('a chunk rejection journals and emits the failed member and syntax error', async t => {
  const root = await mkdtemp(join(tmpdir(), 'chunk-parse-error-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const file = 'member.js', source = 'function calculate(a, a) { return a; }\n';
  await writeFile(join(root, file), source);
  await writeFile(join(root, 'sibling.js'), 'const siblingValue = 1;\n');
  const chunks = join(root, 'chunks.json');
  await writeFile(chunks, JSON.stringify({version: 1, chunks: [{id: 'authored', files: [file, 'sibling.js']}]}));
  const driver = await createChunkRun(root, {chunks, concurrency: 1, minSymbolLength: 1,
    maxInputTokens: 100000, command: 'rename-bindings'}, () => false);
  driver.select([file], new Set());
  const events = [], summary = {completed: 0, skipped: 0, rejected: 0, failedBatches: 0};
  await driver.process(driver.jobs[0], {api: {...select, ...rename, ...checks},
    namer: {name() { throw new Error('No model request is expected'); }}, withSlot: task => task(),
    summary, sources: new Map(), emit: event => events.push(event), phase: () => () => {}});
  const rejected = events.find(event => event.status === 'rejected');
  assert.equal(rejected.file, file);
  assert.match(rejected.parseError, /Duplicate parameter/);
  const records = (await readFile(join(root, '.unminify/rename-bindings/chunks', `${hash('authored')}.jsonl`), 'utf8')).trim().split('\n').map(JSON.parse);
  assert.equal(records.at(-1).file, file);
  assert.equal(records.at(-1).parseError, rejected.parseError);
  assert.equal(await readFile(join(root, file), 'utf8'), source);
});
