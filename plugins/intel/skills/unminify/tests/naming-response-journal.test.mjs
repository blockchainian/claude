import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { openJournal, appendRecord } from '../scripts/journal.mjs';
import { openChunkJournal } from '../scripts/chunk-journal.mjs';

for (const chunk of [false, true]) {
  function fixture(t) {
    const root = mkdtempSync(join(tmpdir(), 'response-journal-'));
    t.after(() => rmSync(root, { recursive: true, force: true }));
    const source = 'const a = 1;';
    writeFileSync(join(root, 'a.js'), source);
    const open = () => chunk ? openChunkJournal(root, 'sample', new Map([['a.js', source]])) : openJournal(root, 'a.js', source, {});
    return { open, journal: open() };
  }
  test(`${chunk ? 'chunk' : 'file'} response writes validate shapes and preserve identical replay`, t => {
    const { journal, open } = fixture(t);
    for (const request of ['', null, 1]) assert.throws(() => journal.response(request, new Map([['a', 'count']])));
    for (const names of [new Map(), new Map([['a', 1]]), new Map([['', 'count']]), new Map([[undefined, 'count']]), []]) assert.throws(() => journal.response('request', names));
    journal.response('request', new Map([['a', 'count']]));
    const before = readFileSync(journal.path, 'utf8');
    journal.response('request', new Map([['a', 'count']]));
    assert.equal(readFileSync(journal.path, 'utf8'), before);
    assert.throws(() => journal.response('request', new Map([['a', 'total']])), /conflicting response/);
    assert.deepEqual(open().replies.get('request'), { a: 'count' });
  });
  test(`${chunk ? 'chunk' : 'file'} response reads reject invalid or conflicting records`, t => {
    for (const record of [
      { request: '', names: { a: 'count' } }, { request: 'request', names: [] },
      { request: 'request', names: { a: 42 } }, { request: 'request', names: { a: 'different' } },
    ]) {
      const { journal, open } = fixture(t);
      journal.response('request', new Map([['a', 'count']]));
      appendRecord(journal.path, { type: 'response', ...record });
      assert.throws(open, /invalid response|conflicting response/);
    }
  });
}
