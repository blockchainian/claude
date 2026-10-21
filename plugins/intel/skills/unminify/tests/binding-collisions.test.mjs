import assert from 'node:assert/strict';
import test from 'node:test';
import { resolveNames, applyRenames, followImports } from '../scripts/rename.mjs';
import { resolveChunkNames, applyChunkRenames, checkChunkRewrite } from '../scripts/chunks.mjs';
import { checkRewrite } from '../scripts/checks.mjs';

// Authored examples use recovered fragments' module imports and separate owners.
for (const [label, source, expected] of [
  ['unused descendant', 'const a = 7; function readLocal() { let amount = 3; return amount; } console.log(a);', 'amount'],
  ['unused ancestor', 'const amount = 7; function readLocal(a) { return a; }', 'amount'],
  ['captured read', 'const a = 7; function readLocal(amount) { return a + amount; }', 'amount2'],
  ['captured write', 'let a = 7; function writeLocal(amount) { a = amount; }', 'amount2'],
  ['captured ancestor read', 'const amount = 7; function readLocal(a) { return a + amount; }', 'amount2'],
  ['captured ancestor write', 'let amount = 7; function writeLocal(a) { amount = a; }', 'amount2'],
  ['same scope', 'const a = 7, amount = 3; console.log(a);', 'amount2'],
  ['shadowed free name', 'const a = 7; function readLocal(amount) { return amount; } console.log(a);', 'amount'],
]) test(`file collision checks binding changes: ${label}`, () => {
  const key = source.indexOf('a =') >= 0 ? source.indexOf('a =') : source.indexOf('(a)') + 1;
  const names = resolveNames(source, new Map([[key, 'amount']]));
  assert.equal(names.get(key), expected);
  assert.equal(checkRewrite(source, applyRenames(source, names)), null);
});

for (const chunk of [false, true]) test(`external imports permit unrelated local names (${chunk ? 'chunk' : 'file'})`, () => {
  const source = 'import { platform as platform2 } from "os";\nfunction readLocal() { let platform = "fixture"; return platform; }\nconsole.log(platform2());';
  if (chunk) {
    const before = new Map([['imports.js', source], ['use.js', 'console.log(platform2());']]);
    const names = resolveChunkNames(before, new Map([['platform2', 'platform']]), { allowImports: true });
    assert.equal(names.get('platform2'), 'platform');
    assert.equal(checkChunkRewrite(before, applyChunkRenames(before, names)), null);
  } else {
    const after = followImports(new Map([['imports.js', source]])).get('imports.js');
    assert.equal(after, source.replace('platform as platform2', 'platform').replace('platform2()', 'platform()'));
    assert.equal(checkRewrite(source, after), null);
  }
});

test('chunk renames release previous spellings for subsequent proposals', () => {
  const before = new Map([['decl.js', 'const a = 7, b = 3;'], ['use.js', 'console.log(a, b);']]);
  const names = resolveChunkNames(before, new Map([['a', 'amount'], ['b', 'a']]));
  assert.deepEqual([...names], [['a', 'amount'], ['b', 'a']]);
  assert.equal(checkChunkRewrite(before, applyChunkRenames(before, names)), null);
});

test('chunk globals shadowed away from the renamed references do not reserve names', () => {
  const before = new Map([['decl.js', 'const a = 7;'], ['use.js', 'function readLocal(console) { return console; } globalThis.result = a;']]);
  const names = resolveChunkNames(before, new Map([['a', 'console']]));
  assert.equal(names.get('a'), 'console');
  assert.equal(checkChunkRewrite(before, applyChunkRenames(before, names)), null);
});

for (const chunk of [false, true]) test(`duplicate package imports keep required suffixes (${chunk ? 'chunk' : 'file'})`, () => {
  const source = 'import { basename as firstBase, basename as secondBase, basename as thirdBase, dirname, dirname as otherDir, join, join as otherJoin } from "path"; console.log(firstBase, secondBase, thirdBase, dirname, otherDir, join, otherJoin);';
  const before = new Map([['imports.js', source]]);
  let after;
  if (chunk) {
    const proposals = new Map([['firstBase', 'basename'], ['secondBase', 'basename'], ['thirdBase', 'basename'], ['otherDir', 'dirname'], ['otherJoin', 'join']]);
    after = applyChunkRenames(before, resolveChunkNames(before, proposals, { allowImports: true }));
    assert.equal(checkChunkRewrite(before, after), null);
  } else {
    after = followImports(before);
    assert.equal(checkRewrite(source, after.get('imports.js')), null);
  }
  assert.equal(after.get('imports.js'), 'import { basename, basename as basename2, basename as basename3, dirname, dirname as dirname2, join, join as join2 } from "path"; console.log(basename, basename2, basename3, dirname, dirname2, join, join2);');
});

for (const [label, reference, expected] of [
  ['read', 'function readLocal(amount) { return a + amount; }', 'amount2'],
  ['assignment', 'function writeLocal(amount) { a = amount; }', 'amount2'],
  ['destructuring assignment', 'function writeLocal(amount) { ({value: a} = amount); }', 'amount2'],
  ['loop target', 'function writeLocal(amount) { for (a of amount) {} }', 'amount2'],
  ['unrelated local', 'function readLocal(amount) { return amount; } console.log(a);', 'amount'],
]) test(`chunk references determine capture: ${label}`, () => {
  const before = new Map([['decl.js', 'let a = 7;'], ['use.js', reference]]);
  const names = resolveChunkNames(before, new Map([['a', 'amount']]));
  assert.equal(names.get('a'), expected);
  assert.equal(checkChunkRewrite(before, applyChunkRenames(before, names)), null);
});

test('chunk references can reuse a renamed program spelling in another file', () => {
  const before = new Map([['first.js', 'const a = 7;'], ['second.js', 'const b = 3; function readBoth() { return a + b; }']]);
  const names = resolveChunkNames(before, new Map([['b', 'amount'], ['a', 'b']]));
  assert.deepEqual([...names], [['b', 'amount'], ['a', 'b']]);
  assert.equal(checkChunkRewrite(before, applyChunkRenames(before, names)), null);
});
