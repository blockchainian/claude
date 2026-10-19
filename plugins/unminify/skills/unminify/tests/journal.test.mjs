import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, appendFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { openJournal, atomicWrite, hash } from '../scripts/journal.mjs';

async function workspace(t) {
  const root = await mkdtemp(join(tmpdir(), 'unminify-journal-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  return root;
}
test('batch records are durable and replaying a record is idempotent', async t => {
  const root = await workspace(t);
  const journal = openJournal(root, 'lib/a.js', 'const a=1;', { shortLen: 3 });
  journal.batch('0:10', new Map([[6, 'answer']]));
  journal.batch('0:10', new Map([[6, 'answer']]));
  const replay = openJournal(root, 'lib/a.js', 'const a=1;', { shortLen: 3 });
  assert.deepEqual(replay.batches.get('0:10'), { '6': 'answer' });
  assert.equal((await readFile(journal.path, 'utf8')).trim().split('\n').length, 2);
  assert.throws(() => replay.batch('0:10', new Map([[6, 'different']])), /conflicting/);
  assert.throws(() => openJournal(root, 'lib/a.js', 'const b=1;', { shortLen: 3 }), /source changed/);
  assert.throws(() => openJournal(root, 'lib/a.js', 'const a=1;', { shortLen: 2 }), /options changed/);
});
test('a killed append loses only its incomplete final line', async t => {
  const root = await workspace(t);
  const journal = openJournal(root, 'a.js', 'let a=1;', {});
  journal.batch('one', new Map([[4, 'count']]));
  await appendFile(journal.path, '{"id":"torn');
  const replay = openJournal(root, 'a.js', 'let a=1;', {});
  replay.batch('two', new Map());
  assert.equal(openJournal(root, 'a.js', 'let a=1;', {}).batches.size, 2);
  await appendFile(journal.path, '{bad}\n');
  assert.throws(() => openJournal(root, 'a.js', 'let a=1;', {}), /JSON/);
});
test('write intent recovers a crash between atomic replacement and completion', async t => {
  const root = await workspace(t);
  const before = 'let a=1;', after = 'let answer=1;';
  const journal = openJournal(root, 'a.js', before, {});
  journal.prepare(before, after, 'renamed');
  await writeFile(join(root, 'a.js'), before, { mode: 0o755 });
  atomicWrite(join(root, 'a.js'), after);
  const replay = openJournal(root, 'a.js', after, {});
  assert.equal(replay.done.status, 'renamed');
  assert.equal(replay.done.hash, hash(after));
  assert.equal(openJournal(root, 'a.js', after, {}).done.status, 'renamed');
  assert.throws(() => openJournal(root, '../escape.js', before, {}), /relative/);
});
