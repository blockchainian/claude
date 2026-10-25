import test from 'node:test';
import assert from 'node:assert/strict';
import { analyzeChunk, resolveChunkNames, applyChunkRenames, checkChunkRewrite } from '../scripts/chunks.mjs';

const sources = new Map([
  ['a.js', 'import { readFile as a } from "node:fs"; const b = 1;'],
  ['b.js', 'function c(a) { return a + b; } console.log(a, c(2));'],
  ['c.js', 'const d = { b }; export { d };'],
]);
const proposals = new Map([['a', 'readFile']]);
const options = { allowImports: true, restoreExternalImports: true, chunkSiblings: true };
const baselineNames = resolveChunkNames(sources, proposals, options);
const baseline = applyChunkRenames(sources, baselineNames);
assert.equal(checkChunkRewrite(sources, baseline), null);
assert.deepEqual([...baseline], [
  ['a.js', 'import { readFile } from "node:fs"; const b = 1;'],
  ['b.js', 'function c(a) { return a + b; } console.log(readFile, c(2));'],
  ['c.js', 'const d = { b }; export { d };'],
]);

test('compact chunk facts preserve baseline edits and binding checks', async () => {
  const api = await import('../scripts/import-chunks.mjs');
  const analysis = await api.analyzeImportChunk(sources, options);
  const names = api.resolveImportChunkNames(analysis, proposals, options);
  assert.deepEqual(names, baselineNames);
  const after = await api.applyImportChunkRenames(sources, names, analysis);
  assert.deepEqual(after, baseline);
  assert.equal(await api.checkImportChunkRewrite(sources, after, analysis), null);
  assert.equal(analysis.files.size, analyzeChunk(sources).files.size);
  assert.doesNotThrow(() => structuredClone(analysis));
  api.releaseImportChunk(analysis);
  assert.equal(analysis.files.size, 0);
});

// Observe real objects at the cache insertion site without changing results.
// Validation must not add whole-chunk trees to the naming analysis.
test('cached validation retains only one whole-chunk Babel analysis', async () => {
  const { registerHooks } = await import('node:module');
  const refs = [], key = `__importChunkObserver${process.pid}`;
  let peak = 0;
  globalThis[key] = analysis => {
    assert.ok([...analysis.files.values()].every(record => record.ast?.type === 'File'));
    refs.push(new WeakRef(analysis));
    peak = Math.max(peak, refs.filter(ref => ref.deref()).length);
  };
  const hooks = registerHooks({
    load(url, context, next) {
      const result = next(url, context);
      if (!url.endsWith('/chunks.mjs?import-chunk-observer')) return result;
      const source = String(result.source);
      const needle = '  inspections.set(sources, {';
      assert.ok(source.includes(needle));
      return { ...result, source: source.replace(needle, `  globalThis[${JSON.stringify(key)}]?.(analysis);\n${needle}`) };
    },
  });
  try {
    const chunkApi = await import('../scripts/chunks.mjs?import-chunk-observer');
    const names = chunkApi.resolveChunkNames(sources, proposals, options);
    const after = chunkApi.applyChunkRenames(sources, names);
    assert.equal(chunkApi.checkChunkRewrite(sources, after), null);
    assert.equal(peak, 1, `observed ${peak} real whole-chunk analyses`);
    if (process.env.UNMINIFY_IMPORT_CHUNK_BASELINE === '1') assert.ok(peak <= 1, `observed ${peak} real whole-chunk analyses`);
  } finally { hooks.deregister(); delete globalThis[key]; }
});

test('compact file records contain no Babel node, path, scope or binding objects', async () => {
  const { analyzeImportChunk, releaseImportChunk } = await import('../scripts/import-chunks.mjs');
  const analysis = await analyzeImportChunk(sources, options);
  const seen = new Set();
  function check(value) {
    if (!value || typeof value !== 'object' || seen.has(value)) return;
    seen.add(value);
    assert.equal(typeof value.type, 'undefined');
    for (const key of ['ast', 'scope', 'parentPath', 'referencePaths', 'identifier']) assert.equal(Object.hasOwn(value, key), false, key);
    const values = value instanceof Map ? [...value.keys(), ...value.values()] : value instanceof Set ? [...value] : Object.values(value);
    for (const child of values) check(child);
  }
  check(analysis);
  releaseImportChunk(analysis);
});
