import test from 'node:test';
import assert from 'node:assert/strict';
import { applyChunkRenames, checkChunkRewrite } from '../scripts/chunks.mjs';
import { withTimings } from '../scripts/timing.mjs';

test('inverted renames and unchanged siblings avoid repeated single-file syntax checks', () => {
  const before = new Map([
    ['entry.js', 'const a = 3; console.log(a);'],
    ['sibling.js', 'function report(value) { return value + 1; }'],
  ]);
  const after = applyChunkRenames(before, new Map([['a', 'amount']]));
  assert.notEqual(after.get('entry.js'), before.get('entry.js'));
  assert.equal(after.get('sibling.js'), before.get('sibling.js'));
  const totals = {};
  assert.equal(withTimings(totals, () => checkChunkRewrite(before, after)), null);
  assert.equal(totals.parse ?? 0, 0);
});

test('inversion still rejects a rename captured by a sibling local binding', () => {
  const before = new Map([
    ['entry.js', 'const a = 3;'],
    ['sibling.js', 'function report(value) { return a + value; }'],
  ]);
  const after = applyChunkRenames(before, new Map([['a', 'value']]));
  assert.equal(checkChunkRewrite(before, after), 'CHUNK_BINDING_CHANGED');
});

test('a renamed file with changed literal bytes still reaches the strict guard', () => {
  const before = new Map([
    ['entry.js', 'const a = 3; console.log(a);'],
    ['sibling.js', 'function report(value) { return value + 1; }'],
  ]);
  const after = applyChunkRenames(before, new Map([['a', 'amount']]));
  after.set('entry.js', after.get('entry.js').replace('= 3;', '= 4;'));
  assert.equal(checkChunkRewrite(before, after), 'entry.js: STRUCTURE_OR_TEXT_CHANGED');
});
