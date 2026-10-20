import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, symlink, rm } from 'node:fs/promises';
import path from 'node:path';
import { tmpdir } from 'node:os';
import { loadChunks, createChunkIgnore } from '../scripts/chunk-manifest.mjs';

async function fixture(t) {
  const parent = await mkdtemp(path.join(tmpdir(), 'unminify-manifest-'));
  t.after(() => rm(parent, { recursive: true, force: true }));
  const root = path.join(parent, 'repo');
  await mkdir(path.join(root, 'src'), { recursive: true });
  for (const file of ['a.js', 'b.js', 'c.js']) await writeFile(path.join(root, 'src', file), 'const a = 1;');
  const manifest = path.join(root, 'chunks.json');
  const write = chunks => writeFile(manifest, JSON.stringify({ version: 1, chunks }));
  return { parent, root, manifest, write };
}
const chunk = (id = 'example', files = ['src/a.js', 'src/b.js']) => ({ id, files });

test('chunk manifests resolve from their directory, canonicalize and fingerprint deterministically', async t => {
  const { root, manifest, write } = await fixture(t);
  await write([chunk('second', ['src/c.js', 'src/b.js'])]);
  const first = await loadChunks(manifest, root);
  assert.deepEqual(first.chunks, [chunk('second', ['src/b.js', 'src/c.js'])]);
  assert.deepEqual([...first.files], ['src/b.js', 'src/c.js']);
  await write([chunk('second', ['./src/b.js', './src/c.js'])]);
  assert.equal((await loadChunks(manifest, root)).fingerprint, first.fingerprint);
  await write([chunk('renamed', ['src/b.js', 'src/c.js'])]);
  assert.notEqual((await loadChunks(manifest, root)).fingerprint, first.fingerprint);
  await mkdir(path.join(root, 'config'));
  const nested = path.join(root, 'config', 'chunks.json');
  // The schema deliberately forbids '..', so valid nested manifests reference descendants.
  await mkdir(path.join(root, 'config', 'src'));
  await writeFile(path.join(root, 'config', 'src', 'a.js'), '');
  await writeFile(path.join(root, 'config', 'src', 'b.js'), '');
  await writeFile(nested, JSON.stringify({ version: 1, chunks: [chunk()] }));
  assert.deepEqual((await loadChunks(nested, root)).chunks, [chunk('example', ['config/src/a.js', 'config/src/b.js'])]);
});

test('manifests may live outside the Git root', async t => {
  const { parent, root } = await fixture(t);
  const manifest = path.join(parent, 'chunks.json');
  await writeFile(manifest, JSON.stringify({ version: 1, chunks: [chunk('example', ['repo/src/a.js', 'repo/src/b.js'])] }));
  assert.deepEqual((await loadChunks(manifest, root)).chunks, [chunk()]);
});

test('missing files, directories, duplicate IDs and canonical file aliases are rejected', async t => {
  const { root, manifest, write } = await fixture(t);
  await symlink(path.join(root, 'src/a.js'), path.join(root, 'src/link.js'));
  for (const [chunks, pattern] of [
    [[chunk('one', ['src/a.js', 'src/missing.js'])], /missing|ENOENT/i],
    [[chunk('one', ['src/a.js', 'src'])], /regular file/i],
    [[chunk('one'), chunk('one', ['src/b.js', 'src/c.js'])], /duplicate.*id/i],
    [[chunk('one'), chunk('two', ['src/b.js', 'src/c.js'])], /multiple chunks|duplicate.*file/i],
    [[chunk('one', ['src/a.js', './src/a.js'])], /duplicate.*file/i],
    [[chunk('one', ['src/a.js', 'src/link.js'])], /duplicate.*file/i],
  ]) {
    await write(chunks);
    await assert.rejects(loadChunks(manifest, root), pattern);
  }
});

test('schema rejects wrong versions, unknown fields, invalid chunks and invalid paths', async t => {
  const { manifest, root } = await fixture(t);
  for (const value of [
    { version: 2, chunks: [chunk()] },
    { version: 1, chunks: [chunk()], extra: true },
    { version: 1, chunks: [{ ...chunk(), extra: true }] },
    { version: 1, chunks: [chunk('', ['src/a.js', 'src/b.js'])] },
    { version: 1, chunks: [chunk('one', ['src/a.js'])] },
    { version: 1, chunks: [chunk('one', ['src/a.js', 'src/a.js'])] },
    ...['../a.js', '/tmp/a.js', 'C:/a.js', 'C:\\a.js', 'src\\a.js'].map(file =>
      ({ version: 1, chunks: [chunk('one', ['src/a.js', file])] })),
  ]) {
    await writeFile(manifest, JSON.stringify(value));
    await assert.rejects(loadChunks(manifest, root), /manifest|schema|path/i);
  }
});

test('symlinks cannot point outside the selected Git root', async t => {
  const { parent, root, manifest, write } = await fixture(t);
  await writeFile(path.join(parent, 'outside.js'), '');
  await symlink(path.join(parent, 'outside.js'), path.join(root, 'src/link.js'));
  await write([chunk('one', ['src/a.js', 'src/link.js'])]);
  await assert.rejects(loadChunks(manifest, root), /outside.*root/i);
});

test('ignore predicates preserve listFiles directory, glob, negation and custom-root semantics', async t => {
  const { root } = await fixture(t);
  const absent = await createChunkIgnore(root);
  assert.equal(absent('src/a.js'), false);
  await writeFile(path.join(root, '.ignore'), 'vendor/\n*.min.*\n!allow.min.js\n');
  const ignored = await createChunkIgnore(root);
  assert.equal(ignored('vendor/deep.js'), true);
  assert.equal(ignored(path.join(root, 'src', 'vendor', 'deep.js')), true);
  assert.equal(ignored('src/drop.min.js'), true);
  assert.equal(ignored('src/allow.min.js'), false);
  await writeFile(path.join(root, 'src', 'custom.ignore'), '*.js\n!a.js\n');
  const custom = await createChunkIgnore(root, path.join(root, 'src', 'custom.ignore'));
  assert.equal(custom('src/a.js'), false);
  assert.equal(custom('src/b.js'), true);
  assert.equal(custom('other/b.js'), false);
  await assert.rejects(createChunkIgnore(root, path.join(root, 'missing.ignore')), /ENOENT/);
});

test('ignored files remain members of an explicit chunk and malformed JSON fails', async t => {
  const { root, manifest, write } = await fixture(t);
  await writeFile(path.join(root, '.ignore'), 'src/b.js\n');
  await write([chunk()]);
  const loaded = await loadChunks(manifest, root);
  const ignored = await createChunkIgnore(root);
  assert.equal(loaded.files.has('src/b.js'), true);
  assert.equal(ignored('src/b.js'), true);
  await writeFile(manifest, '{"version":');
  await assert.rejects(loadChunks(manifest, root), SyntaxError);
});
