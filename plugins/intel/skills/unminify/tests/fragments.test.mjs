import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { openJournal } from '../scripts/journal.mjs';
import { parse } from '../scripts/node_modules/@babel/parser/lib/index.js';
import traverseModule from '../scripts/node_modules/@babel/traverse/lib/index.js';
import { listFiles, planFile, renderBatch } from '../scripts/select.mjs';
import { resolveNames, applyRenames, followImports } from '../scripts/rename.mjs';
import { indexFragments, loadFragmentIndex } from '../scripts/fragments.mjs';
import { checkRewrite } from '../scripts/checks.mjs';
const traverse = traverseModule.default ?? traverseModule;
const fixtures = new Map(await Promise.all(['first', 'middle', 'last'].map(async name =>
  [name, await readFile(new URL(`./fixtures/chunk-${name}.mjs`, import.meta.url), 'utf8')])));
const fragments = indexFragments(fixtures);
function bindings(source) {
  const result = new Map();
  traverse(parse(source, { sourceType: 'unambiguous', allowUndeclaredExports: true }), {
    Scope(p) { for (const b of Object.values(p.scope.bindings)) result.set(b.identifier.start, b); },
  });
  return result;
}
for (const [name, source] of fixtures) test(`${name} fragment preserves program bindings and renames locals`, () => {
  const bs = bindings(source);
  const options = { fragment: fragments.has(name) };
  assert(options.fragment);
  const keys = planFile(source, options).stages.flat().flatMap(batch => {
    assert.doesNotThrow(() => renderBatch(source, batch));
    return batch.keys;
  });
  const expected = [...bs].filter(([,b]) => !b.scope.path.isProgram() && b.identifier.name.length < 4).map(([k]) => k);
  assert(expected.length > 0);
  assert.deepEqual(keys.sort((a,b)=>a-b), expected.sort((a,b)=>a-b));
  const proposals = new Map([...bs].map(([key]) => [key, `readable${key}`]));
  const resolved = resolveNames(source, proposals, options);
  for (const [key,b] of bs) assert.equal(resolved.has(key), !b.scope.path.isProgram());
  const after = applyRenames(source, resolved);
  assert.notEqual(after, source);
  assert.equal(checkRewrite(source, after, options), null);
  for (const [key,b] of bs) if (b.scope.path.isProgram()) {
    const unsafe = applyRenames(source, new Map([[key, `unsafe${key}`]]));
    assert.equal(checkRewrite(source, unsafe, options), 'FRAGMENT_TOP_LEVEL_CHANGED');
  }
});
test('followImports leaves the first fragment import locals intact', () => {
  const first = fixtures.get('first');
  const files = new Map([
    ['src/slices/first.js', first],
    ['src/slices/registry.js', 'const registerEntry = 1; export { registerEntry as Wp };'],
    ['src/slices/last.js', fixtures.get('last')],
  ]);
  assert.equal(followImports(files, undefined, indexFragments(files)).size, 0);
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
  assert.notEqual(checkRewrite(source, source.replace('Jq,', 'anotherName as Jq,')), null);
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

test('real standalone CommonJS fixture offers and renames program bindings', async () => {
  const source = await readFile(new URL('./fixtures/preact-hooks.cjs', import.meta.url), 'utf8');
  assert(!indexFragments(new Map([...fixtures, ['standalone.cjs', source]])).has('standalone.cjs'));
  const bs = bindings(source);
  const top = [...bs].filter(([, b]) => b.scope.path.isProgram() && b.identifier.name.length < 4);
  assert(top.length > 0);
  const keys = new Set(planFile(source).stages.flat().flatMap(batch => batch.keys));
  for (const [key] of top) assert(keys.has(key));
  const proposed = new Map(top.map(([key]) => [key, `readable${key}`]));
  const resolved = resolveNames(source, proposed);
  assert.equal(resolved.size, top.length);
  assert.equal(checkRewrite(source, applyRenames(source, resolved)), null);
});

test('unused import aliases and sibling declarations share protection without a free read', () => {
  const sources = new Map([
    ['first.js', 'import { item as Zp } from "./dependency.js";'],
    ['middle.js', 'function Zp(e) { return e + 2; }'],
  ]);
  assert.deepEqual(indexFragments(sources), new Set(sources.keys()));
});

test('independent CommonJS scripts ignore unrelated short-name collisions', () => {
  const sources = new Map([
    ['a.js', 'var a = require("fs"); function b(c) { return a.readFileSync(c); } module.exports = b;'],
    ['b.js', 'var a = 4; function b(c) { return c + a; } module.exports = b;'],
    ['other.js', 'function c(e) { return b(e); }'],
  ]);
  const fragments = indexFragments(sources);
  assert(!fragments.has('a.js'));
  assert(!fragments.has('b.js'));
  assert(fragments.has('other.js'));
});

test('the repository index finds siblings outside a selected directory', async t => {
  const root = await mkdtemp(join(tmpdir(), 'unminify-index-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  execFileSync('git', ['init', root], { stdio: 'ignore' });
  await writeFile(join(root, 'selected.js'), 'function Zp(e) { return e * 7; }');
  await writeFile(join(root, 'sibling.js'), 'console.log(Zp(3));');
  assert(loadFragmentIndex(root, ['selected.js']).has('selected.js'));
});

test('without a Git repository the index uses only the supplied files', async t => {
  const root = await mkdtemp(join(tmpdir(), 'unminify-no-git-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  await writeFile(join(root, 'selected.js'), 'function Zp(e) { return e * 7; }');
  await writeFile(join(root, 'sibling.js'), 'console.log(Zp(3));');
  assert(!loadFragmentIndex(root, ['selected.js']).has('selected.js'));
  assert(loadFragmentIndex(root, ['selected.js', 'sibling.js']).has('selected.js'));
});


test('CLI replay protects a sibling name outside its input and still renames a local', async t => {
  const root = await mkdtemp(join(tmpdir(), 'unminify-replay-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  execFileSync('git', ['init', root], { stdio: 'ignore' });
  const source = 'function Zp(e) { return e + 7; }';
  const sibling = 'console.log(Zp(4));';
  await writeFile(join(root, 'selected.js'), source);
  await writeFile(join(root, 'sibling.js'), sibling);
  const options = { minSymbolLength: 4, maxInputTokens: 100000 };
  const batch = planFile(source, { ...options, fragment: true }).stages.flat()[0];
  const journal = openJournal(root, 'selected.js', source, options);
  journal.batch(batch.id, new Map([[source.indexOf('Zp'), 'unsafeTop'], [batch.keys[0], 'value']]));
  const output = execFileSync(process.execPath, [fileURLToPath(new URL('../scripts/cli.mjs', import.meta.url)),
    join(root, 'selected.js')], { encoding: 'utf8', timeout: 10000 });
  const summary = output.trim().split('\n').map(line => JSON.parse(line)).find(record => record.type === 'summary');
  assert.equal(summary.exitCode, 0);
  assert.equal(summary.requests, 0);
  assert.equal(summary.completed, 1);
  assert.equal(await readFile(join(root, 'selected.js'), 'utf8'), 'function Zp(value) { return value + 7; }');
  assert.equal(await readFile(join(root, 'sibling.js'), 'utf8'), sibling);
});
