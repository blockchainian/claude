import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readFileSync, rmSync, appendFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { openChunkJournal, recoverChunkJournals } from '../scripts/chunk-journal.mjs';

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'chunk-journal-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const sources = new Map([['a.js', 'const a = 1;'], ['b.js', 'consume(a);']]);
  for (const [file, source] of sources) writeFileSync(join(root, file), source);
  return { root, sources, options: { manifest: 'fingerprint', siblings: true } };
}
const output = () => new Map([['a.js', 'const count = 1;'], ['b.js', 'consume(count);']]);

test('accepted replies survive reopening with stable batch IDs', t => {
  const { root, sources, options } = fixture(t);
  const journal = openChunkJournal(root, 'chunk-a', sources, options);
  journal.batch('top:0', new Map([[0, 'count'], ['binding:1', 'total']]));
  appendFileSync(journal.path, '{"incomplete"');
  const resumed = openChunkJournal(root, 'chunk-a', sources, options);
  assert.deepEqual(resumed.batches.get('top:0'), { 0: 'count', 'binding:1': 'total' });
  assert.throws(() => resumed.batch('top:0', new Map([[0, 'other']])), /conflicting batch/);
});

test('pending transaction resumes a half-written chunk before parsing', t => {
  const { root, sources, options } = fixture(t);
  const journal = openChunkJournal(root, 'chunk-a', sources, options);
  journal.prepare(sources, output(), 'changed');
  writeFileSync(join(root, 'a.js'), output().get('a.js'));
  assert.equal(recoverChunkJournals(root), 1);
  for (const [file, source] of output()) assert.equal(readFileSync(join(root, file), 'utf8'), source);
  const resumed = openChunkJournal(root, 'chunk-a', output(), options);
  assert.equal(resumed.done.status, 'changed');
  assert.equal(recoverChunkJournals(root), 0);
});

test('recovery validates every source before writing any unfinished file', t => {
  const { root, sources, options } = fixture(t);
  openChunkJournal(root, 'chunk-a', sources, options).prepare(sources, output(), 'changed');
  writeFileSync(join(root, 'b.js'), 'externalEdit();');
  assert.throws(() => recoverChunkJournals(root), /source changed/);
  assert.equal(readFileSync(join(root, 'a.js'), 'utf8'), sources.get('a.js'));
});

test('source, membership and options mismatches are rejected including completed journals', t => {
  const { root, sources, options } = fixture(t);
  const journal = openChunkJournal(root, 'chunk-a', sources, options);
  assert.throws(() => openChunkJournal(root, 'chunk-a', sources, { ...options, siblings: false }), /options changed/);
  assert.throws(() => openChunkJournal(root, 'chunk-a', new Map([['a.js', sources.get('a.js')]]), options), /membership changed/);
  assert.throws(() => openChunkJournal(root, 'chunk-a', output(), options), /source changed/);
  journal.commit(output());
  assert.throws(() => openChunkJournal(root, 'chunk-a', output(), { ...options, siblings: false }), /options changed/);
  writeFileSync(join(root, 'b.js'), 'externalEdit();');
  assert.throws(() => openChunkJournal(root, 'chunk-a', output(), options), /source changed/);
});

test('commit interrupted between files recovers exactly the planned output', t => {
  const { root, sources, options } = fixture(t);
  const journal = openChunkJournal(root, 'chunk-a', sources, options);
  let count = 0;
  assert.throws(() => journal.commit(output(), { write(file, source) {
    if (++count === 2) throw new Error('interrupted');
    writeFileSync(file, source);
  } }), /interrupted/);
  recoverChunkJournals(root);
  for (const [file, source] of output()) assert.equal(readFileSync(join(root, file), 'utf8'), source);
});

test('journal rejects paths outside root', t => {
  const { root, options } = fixture(t);
  assert.throws(() => openChunkJournal(root, 'chunk-a', new Map([['../outside.js', 'x']]), options), /relative repository path/);
});

test('recovery checks all pending chunks before writing the first chunk', t => {
  const { root, sources, options } = fixture(t);
  const second = new Map([['c.js', 'const b = 2;'], ['d.js', 'consume(b);']]);
  for (const [file, source] of second) writeFileSync(join(root, file), source);
  openChunkJournal(root, 'chunk-a', sources, options).prepare(sources, output());
  openChunkJournal(root, 'chunk-b', second, options).prepare(second, new Map([['c.js', 'const size = 2;'], ['d.js', 'consume(size);']]));
  writeFileSync(join(root, 'd.js'), 'externalEdit();');
  assert.throws(() => recoverChunkJournals(root), /source changed/);
  assert.equal(readFileSync(join(root, 'a.js'), 'utf8'), sources.get('a.js'));
  assert.equal(readFileSync(join(root, 'c.js'), 'utf8'), second.get('c.js'));
});

test('pending recovery rejects changed configuration before any write', t => {
  const { root, sources, options } = fixture(t);
  openChunkJournal(root, 'chunk-a', sources, options).prepare(sources, output());
  assert.throws(() => recoverChunkJournals(root, { ...options, siblings: false }), /options changed/);
  assert.equal(readFileSync(join(root, 'a.js'), 'utf8'), sources.get('a.js'));
  assert.equal(recoverChunkJournals(root, id => { assert.equal(id, 'chunk-a'); return options; }), 1);
});

test('options key ordering is stable and unchanged members are never written', t => {
  const { root, sources, options } = fixture(t);
  openChunkJournal(root, 'chunk-a', sources, options);
  const journal = openChunkJournal(root, 'chunk-a', sources, { siblings: true, manifest: 'fingerprint' });
  const after = new Map(sources);
  after.set('a.js', 'const count = 1;');
  const written = [];
  journal.commit(after, { write(file, source) { written.push(file); writeFileSync(file, source); } });
  assert.deepEqual(written, [join(root, 'a.js')]);
  assert.equal(openChunkJournal(root, 'chunk-a', after, options).done.status, 'changed');
});

test('journal accepts a root-contained filename beginning with two dots', t => {
  const { root, options } = fixture(t);
  writeFileSync(join(root, '..member.js'), 'const a = 1;');
  assert.doesNotThrow(() => openChunkJournal(root, 'dot-member', new Map([['..member.js', 'const a = 1;']]), options));
});
