import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, rm, readdir, readFile, utimes } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { runCheckSyntax } from '../scripts/check-syntax.mjs';
import { transformSyntax } from '../scripts/syntax/runner.mjs';
import { mergeExternalImports, importMergeVersion } from '../scripts/merge-imports.mjs';
import { syntaxFingerprint } from '../scripts/syntax/stage.mjs';

async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), 'unminify-check-syntax-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  await writeFile(join(root, 'entry.js'), 'export const value = true;');
  const options = { files: ['entry.js'], fragmentContext: { fragments: new Set(), topLevelBindings: new Set(['value']) },
    chunks: [], ignored: [], syntaxFingerprint, importMergeVersion };
  let checks = 0;
  async function check(inputs = options) {
    checks++;
    const pending = [];
    for (const file of inputs.files) {
      const source = await readFile(join(root, file), 'utf8');
      const restored = transformSyntax(source, { fragment: inputs.fragmentContext.fragments.has(file), fragmentContext: inputs.fragmentContext });
      const merged = mergeExternalImports(new Map([[file, restored.source]]));
      if (source !== merged.get(file)) pending.push(file);
    }
    return pending;
  }
  return { root, options, check, checks: () => checks };
}

test('successful real syntax check is reused across calls and saved without source text', async t => {
  const { root, options, check, checks } = await fixture(t);
  assert.deepEqual(await runCheckSyntax(root, options, check), { cached: false, pending: [] });
  assert.deepEqual(await runCheckSyntax(root, options, check), { cached: true, pending: [] });
  assert.equal(checks(), 1);
  const cache = join(root, '.unminify', 'rename-bindings', 'check-syntax');
  const files = await readdir(cache);
  assert.equal(files.length, 1);
  assert(!String(await readFile(join(cache, files[0]))).includes('export const'));
});

test('source bytes invalidate cache even when size and mtime are unchanged', async t => {
  const { root, options, check, checks } = await fixture(t);
  await utimes(join(root, 'entry.js'), 1, 1);
  await runCheckSyntax(root, options, check);
  await writeFile(join(root, 'entry.js'), 'export const other = true;');
  await utimes(join(root, 'entry.js'), 1, 1);
  assert.equal((await runCheckSyntax(root, options, check)).cached, false);
  assert.equal(checks(), 2);
});

test('group records coexist and a changed group leaves an unchanged group reusable', async t => {
  const { root, options, check, checks } = await fixture(t);
  await writeFile(join(root, 'second.js'), 'export const second = true;');
  const first = { ...options, scope: 'first' };
  const second = { ...options, scope: 'second', files: ['second.js'] };
  await runCheckSyntax(root, first, check);
  await runCheckSyntax(root, second, () => check(second));
  await writeFile(join(root, 'entry.js'), 'export const other = true;');
  assert.equal((await runCheckSyntax(root, first, check)).cached, false);
  assert.equal((await runCheckSyntax(root, second, () => check(second))).cached, true);
  assert.equal(checks(), 3);
});

test('chunk member order is part of the verdict while set insertion order is not', async t => {
  const { root, options, check } = await fixture(t);
  await writeFile(join(root, 'second.js'), 'export const second = true;');
  const ordered = { ...options, files: ['entry.js', 'second.js'], chunks: [{ id: 'shared', files: ['entry.js', 'second.js'] }],
    fragmentContext: { fragments: new Set(['entry.js', 'second.js']), topLevelBindings: new Set(['value', 'second']) } };
  await runCheckSyntax(root, ordered, () => check(ordered));
  const setsReordered = { ...ordered, files: ['second.js', 'entry.js'],
    fragmentContext: { fragments: new Set(['second.js', 'entry.js']), topLevelBindings: new Set(['second', 'value']) } };
  assert.equal((await runCheckSyntax(root, setsReordered, () => check(setsReordered))).cached, true);
  const reversed = { ...ordered, chunks: [{ id: 'shared', files: ['second.js', 'entry.js'] }] };
  assert.equal((await runCheckSyntax(root, reversed, () => check(reversed))).cached, false);
});

test('torn cache data is recomputed by the real checker', async t => {
  const { root, options, check, checks } = await fixture(t);
  await runCheckSyntax(root, options, check);
  const directory = join(root, '.unminify', 'rename-bindings', 'check-syntax');
  const [file] = await readdir(directory);
  await writeFile(join(directory, file), '{');
  assert.equal((await runCheckSyntax(root, options, check)).cached, false);
  assert.equal(checks(), 2);
});

test('file membership, chunk grouping, ignored files, context and versions invalidate cache', async t => {
  const { root, options, check } = await fixture(t);
  await writeFile(join(root, 'second.js'), 'export const second = true;');
  await runCheckSyntax(root, options, check);
  const changes = [
    { files: ['entry.js', 'second.js'] },
    { chunks: [{ id: 'shared', files: ['entry.js'] }] },
    { ignored: ['entry.js'] },
    { fragmentContext: { ...options.fragmentContext, fragments: new Set(['entry.js']) } },
    { fragmentContext: { ...options.fragmentContext, topLevelBindings: new Set(['value', 'undefined']) } },
    { syntaxFingerprint: `${syntaxFingerprint}changed` },
    { importMergeVersion: importMergeVersion + 1 },
  ];
  for (const change of changes) {
    const changed = { ...options, ...change };
    assert.equal((await runCheckSyntax(root, changed, () => check(changed))).cached, false);
  }
});

test('unrelated sibling binding and fragment changes retain a verified group', async t => {
  const { root, options, check, checks } = await fixture(t);
  await runCheckSyntax(root, options, check);
  const unrelated = { ...options, fragmentContext: {
    fragments: new Set(['sibling.js']), topLevelBindings: new Set(['value', 'renamedSibling']) } };
  assert.equal((await runCheckSyntax(root, unrelated, () => check(unrelated))).cached, true);
  assert.equal(checks(), 1);
});

test('NaN shadowing invalidates the comparison guard verdict', async t => {
  const { root, options, check } = await fixture(t);
  await writeFile(join(root, 'entry.js'), 'export const value = NaN === candidate;');
  const shadowed = { ...options, fragmentContext: { fragments: new Set(['entry.js']), topLevelBindings: new Set(['NaN']) } };
  assert.deepEqual((await runCheckSyntax(root, shadowed, () => check(shadowed))).pending, []);
  const intrinsic = { ...shadowed, fragmentContext: { ...shadowed.fragmentContext, topLevelBindings: new Set() } };
  assert.deepEqual(await runCheckSyntax(root, intrinsic, () => check(intrinsic)), { cached: false, pending: ['entry.js'] });
});

test('pending syntax and thrown failures never authorize a cache hit', async t => {
  const { root, options, check, checks } = await fixture(t);
  await writeFile(join(root, 'entry.js'), 'export const value = !0;');
  for (let index = 0; index < 2; index++) {
    assert.deepEqual(await runCheckSyntax(root, options, check), { cached: false, pending: ['entry.js'] });
  }
  assert.equal(checks(), 2);
  await writeFile(join(root, 'entry.js'), 'export const = ;');
  await assert.rejects(runCheckSyntax(root, options, check));
  await assert.rejects(runCheckSyntax(root, options, check));
  assert.equal(checks(), 4);
});

test('changing source during successful checking does not cache unverified bytes', async t => {
  const { root, options, check, checks } = await fixture(t);
  const result = await runCheckSyntax(root, options, async () => {
    const pending = await check();
    await writeFile(join(root, 'entry.js'), 'export const other = true;');
    return pending;
  });
  assert.equal(result.cached, false);
  assert.equal((await runCheckSyntax(root, options, check)).cached, false);
  assert.equal(checks(), 2);
});
