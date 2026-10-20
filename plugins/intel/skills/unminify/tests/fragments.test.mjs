import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { parse } from '../scripts/node_modules/@babel/parser/lib/index.js';
import traverseModule from '../scripts/node_modules/@babel/traverse/lib/index.js';
import { listFiles, planFile, renderBatch } from '../scripts/select.mjs';
import { resolveNames, applyRenames, followImports } from '../scripts/rename.mjs';
import { checkRewrite } from '../scripts/checks.mjs';
const traverse = traverseModule.default ?? traverseModule;
const fixtures = new Map(await Promise.all(['first', 'middle', 'last'].map(async name =>
  [name, await readFile(new URL(`./fixtures/chunk-${name}.mjs`, import.meta.url), 'utf8')])));
function bindings(source) {
  const result = new Map();
  traverse(parse(source, { sourceType: 'unambiguous', allowUndeclaredExports: true }), {
    Scope(p) { for (const b of Object.values(p.scope.bindings)) result.set(b.identifier.start, b); },
  });
  return result;
}
for (const [name, source] of fixtures) test(`${name} fragment preserves program bindings and renames locals`, () => {
  const bs = bindings(source);
  const keys = planFile(source).stages.flat().flatMap(batch => {
    assert.doesNotThrow(() => renderBatch(source, batch));
    return batch.keys;
  });
  const expected = [...bs].filter(([,b]) => !b.scope.path.isProgram() && b.identifier.name.length < 4).map(([k]) => k);
  assert(expected.length > 0);
  assert.deepEqual(keys.sort((a,b)=>a-b), expected.sort((a,b)=>a-b));
  const proposals = new Map([...bs].map(([key]) => [key, `readable${key}`]));
  const resolved = resolveNames(source, proposals);
  for (const [key,b] of bs) assert.equal(resolved.has(key), !b.scope.path.isProgram());
  const after = applyRenames(source, resolved);
  assert.notEqual(after, source);
  assert.equal(checkRewrite(source, after), null);
  for (const [key,b] of bs) if (b.scope.path.isProgram()) {
    const unsafe = applyRenames(source, new Map([[key, `unsafe${key}`]]));
    assert.equal(checkRewrite(source, unsafe), 'FRAGMENT_TOP_LEVEL_CHANGED');
  }
});
test('followImports leaves the first fragment import locals intact', () => {
  const first = fixtures.get('first');
  const files = new Map([
    ['src/services/remote/headless/first.js', first],
    ['src/services/remote/deviceKey.js', 'const registerDevice = 1; export { registerDevice as B5t };'],
    ['src/services/remote/headless/last.js', fixtures.get('last')],
  ]);
  assert.equal(followImports(files).size, 0);
});
test('complete module fixture still offers and renames top-level bindings', async () => {
  const source = await readFile(new URL('./fixtures/preact-hooks.mjs', import.meta.url), 'utf8');
  const bs = bindings(source);
  const keys = planFile(source).stages.flat().flatMap(b => b.keys);
  assert(keys.some(key => bs.get(key).scope.path.isProgram()));
  const resolved = resolveNames(source, new Map(keys.map(key => [key, `readable${key}`])));
  assert([...resolved.keys()].some(key => bs.get(key).scope.path.isProgram()));
  assert.equal(checkRewrite(source, applyRenames(source, resolved)), null);
});
test('an unbound final export passes unchanged and remains bound to the sibling name', () => {
  const source = fixtures.get('last');
  assert.equal(checkRewrite(source, source), null);
  assert.deepEqual(followImports(new Map([['last.js', source]])), new Map());
  assert.notEqual(checkRewrite(source, source.replace('  sQn,', '  anotherName as sQn,')), null);
});
test('unused bindings in a module with exports remain eligible', () => {
  const source = 'import { remote as x } from "./remote.js"; const a = 1; export {};';
  const bs = bindings(source);
  const proposed = new Map([...bs].map(([key]) => [key, `readable${key}`]));
  assert.equal(resolveNames(source, proposed).size, 2);
  assert.equal(planFile(source).stages.flat().flatMap(b => b.keys).length, 1);
});
test('file selection includes final fragments with undeclared exports', async t => {
  const root = await mkdtemp(join(tmpdir(), 'unminify-fragments-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  execFileSync('git', ['init', root], { stdio: 'ignore' });
  for (const [name, source] of fixtures) await writeFile(join(root, `${name}.mjs`), source);
  assert.deepEqual(await listFiles(root), ['first.mjs', 'last.mjs', 'middle.mjs']);
});
