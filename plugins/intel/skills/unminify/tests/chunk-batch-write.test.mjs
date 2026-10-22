import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readFileSync, rmSync, appendFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { openChunkJournal } from '../scripts/chunk-journal.mjs';

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'chunk-batches-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const sources = new Map([['entry.js', 'const a = 3;'], ['use.js', 'console.log(a);']]);
  for (const [file, source] of sources) writeFileSync(join(root, file), source);
  const open = () => openChunkJournal(root, 'shared', sources, { manifest: 'authored' });
  return { open, journal: open() };
}
const names = value => new Map([['entry.js:6', value]]);

test('packed batches retain the exact line records and replay after reopening', t => {
  const { journal, open } = fixture(t);
  journal.batchMany([['chunk:100000:a', names('amount')], ['local:use.js:batch:1', new Map([[12, 'item']])]]);
  const records = readFileSync(journal.path, 'utf8').trim().split('\n').map(line => JSON.parse(line));
  assert.deepEqual(records.slice(1), [
    { type: 'chunk-batch', id: 'chunk:100000:a', names: { 'entry.js:6': 'amount' } },
    { type: 'chunk-batch', id: 'local:use.js:batch:1', names: { 12: 'item' } },
  ]);
  const resumed = open();
  assert.deepEqual(resumed.batches, journal.batches);
  const text = readFileSync(journal.path, 'utf8');
  resumed.batch('chunk:100000:a', names('amount'));
  resumed.batchMany([['local:use.js:batch:1', new Map([[12, 'item']])]]);
  assert.equal(readFileSync(journal.path, 'utf8'), text);
});

test('all existing conflicts are rejected before any proposed batch reaches disk or state', t => {
  const { journal, open } = fixture(t);
  journal.batch('existing', names('amount'));
  const text = readFileSync(journal.path, 'utf8');
  assert.throws(() => journal.batchMany([['fresh', names('size')], ['existing', names('other')]]), /conflicting batch: existing/);
  assert.equal(readFileSync(journal.path, 'utf8'), text);
  assert.equal(journal.batches.has('fresh'), false);
  assert.equal(open().batches.has('fresh'), false);
});

test('duplicate input IDs coalesce equal maps and reject conflicting maps before writing', t => {
  const { journal } = fixture(t);
  const text = readFileSync(journal.path, 'utf8');
  assert.throws(() => journal.batchMany([['fresh', names('size')], ['fresh', names('other')]]), /conflicting batch: fresh/);
  assert.equal(readFileSync(journal.path, 'utf8'), text);
  assert.equal(journal.batches.size, 0);
  journal.batchMany([['fresh', names('size')], ['fresh', names('size')]]);
  assert.equal(readFileSync(journal.path, 'utf8').trim().split('\n').length, 2);
  assert.deepEqual(journal.batches.get('fresh'), { 'entry.js:6': 'size' });
});

test('invalid later IDs do not persist earlier valid entries and empty batches are no-ops', t => {
  const { journal } = fixture(t);
  const text = readFileSync(journal.path, 'utf8');
  assert.throws(() => journal.batchMany([['fresh', names('size')], [17, names('other')]]), /string id/);
  journal.batchMany([]);
  assert.equal(readFileSync(journal.path, 'utf8'), text);
  assert.equal(journal.batches.size, 0);
});

test('batchMany reuses legacy single-batch records and repairs torn trailing records on replay', t => {
  const { journal, open } = fixture(t);
  journal.batch('legacy', names('amount'));
  journal.batchMany([['legacy', names('amount')], ['fresh', names('size')]]);
  appendFileSync(journal.path, '{"type":"chunk-batch","id":"torn"');
  const resumed = open();
  assert.deepEqual([...resumed.batches.keys()], ['legacy', 'fresh']);
  assert.deepEqual(resumed.batches.get('fresh'), { 'entry.js:6': 'size' });
  assert(readFileSync(journal.path, 'utf8').endsWith('\n'));
});
