import assert from 'node:assert/strict';
import test from 'node:test';
import { followImports } from '../scripts/rename.mjs';
import { analyzeChunk, resolveChunkNames, applyChunkRenames, checkChunkRewrite } from '../scripts/chunks.mjs';
import { checkRewrite } from '../scripts/checks.mjs';

// All inputs are authored here, independent of upstream source.
function restore(before, chunk) {
  if (!chunk) return new Map([...before, ...followImports(before)]);
  const proposals = new Map();
  for (const [name, record] of analyzeChunk(before).bindings) {
    const path = record.declarations[0].binding.path;
    if (path.isImportSpecifier()) proposals.set(name, path.node.imported.name);
  }
  return applyChunkRenames(before, resolveChunkNames(before, proposals, { allowImports: true, restoreExternalImports: true }));
}

for (const chunk of [false, true]) {
  for (const [label, imports, uses, expected] of [
    ['most references win', 'import { join as p } from "path/posix"; import { join as j } from "path";', 'console.log(p, j, j, j);', 'console.log(posixJoin, join, join, join);'],
    ['equal counts retain order', 'import { join as p } from "path/posix"; import { join as j } from "path";', 'console.log(p, j);', 'console.log(join, pathJoin);'],
    ['ordinary binding keeps ownership', 'import { join as p } from "path/posix"; import { join as j } from "path"; const join = 0;', 'console.log(p, j, j, join);', 'console.log(posixJoin, pathJoin, pathJoin, join);'],
    ['same source keeps numeric suffixes', 'import { join as p, join as j } from "path";', 'console.log(p, j, j);', 'console.log(join, join2, join2);'],
    ['existing import can yield ownership', 'import { join } from "path/posix"; import { join as j } from "path";', 'console.log(join, j, j);', 'console.log(posixJoin, join, join);'],
  ]) test(`${chunk ? 'chunk' : 'file'} import ownership: ${label}`, () => {
    const before = chunk ? new Map([['imports.js', imports], ['uses.js', uses]]) : new Map([['main.js', `${imports}\n${uses}`]]);
    const after = restore(before, chunk);
    assert.ok([...after.values()].some(source => source.includes(expected)));
    assert.equal(chunk ? checkChunkRewrite(before, after) : checkRewrite(before.get('main.js'), after.get('main.js')), null);
    assert.deepEqual(restore(after, chunk), after);
  });
}

test('chunk ownership counts references across all sibling files and ignores local shadows', () => {
  const before = new Map([
    ['imports.js', 'import { join as p } from "path/posix"; import { join as j } from "path"; console.log(p, p);'],
    ['one.js', 'console.log(j, j); function local(p) { return [p, p, p, p]; }'],
    ['two.js', 'console.log(j);'],
  ]);
  const after = restore(before, true);
  assert.equal(after.get('imports.js'), 'import { join as posixJoin } from "path/posix"; import { join } from "path"; console.log(posixJoin, posixJoin);');
  assert.equal(after.get('one.js'), 'console.log(join, join); function local(p) { return [p, p, p, p]; }');
  assert.equal(checkChunkRewrite(before, after), null);
});
