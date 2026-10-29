import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, writeFile, rm, access } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { executeMoves } from '../scripts/file-moves.mjs';
import { buildDependencyGraph } from '../scripts/dependency-graph.mjs';

async function fixture(t) {
  const root = await mkdtemp(path.join(tmpdir(), 'unminify-file-moves-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  await mkdir(path.join(root, 'src'));
  const moves = await Promise.all(['290736', '159098'].map(async id => {
    const source = await readFile(new URL(`./fixtures/file-moves/${id}.js`, import.meta.url), 'utf8');
    await writeFile(path.join(root, `src/${id}.js`), source);
    return { from: `src/${id}.js`, to: `animation/${id}.js`, source, imports: [] };
  }));
  return { root, moves };
}

test('moves real webpack files without changing numbered references, ignores other files, updates only selected chunk paths', async t => {
  const { root, moves } = await fixture(t);
  await writeFile(path.join(root, 'src/ignored.js'), 'unchanged');
  const manifest = { version: 1, chunks: [{ id: 'a', files: ['src/290736.js', 'src/159098.js', 'src/ignored.js'] }] };
  await writeFile(path.join(root, 'chunks.json'), JSON.stringify(manifest));
  const result = await executeMoves(root, { moves, chunks: 'chunks.json' });
  assert.equal(result.complete, true);
  for (const move of moves) {
    assert.equal(await readFile(path.join(root, move.to), 'utf8'), move.source);
    await assert.rejects(access(path.join(root, move.from)), { code: 'ENOENT' });
  }
  assert.equal(await readFile(path.join(root, 'src/ignored.js'), 'utf8'), 'unchanged');
  assert.deepEqual(JSON.parse(await readFile(path.join(root, 'chunks.json'), 'utf8')).chunks[0].files, ['animation/290736.js', 'animation/159098.js', 'src/ignored.js']);
});

test('rewrites cached relative literal offsets and preserves unselected targets after moving the importer', async t => {
  const { root, moves } = await fixture(t);
  // These declarations are copied from the actual parser module used by this skill.
  const source = await readFile(new URL('../scripts/checks.mjs', import.meta.url), 'utf8');
  const match = /from (['"])(\.\/[^'"]+)\1/.exec(source);
  assert.ok(match, 'real upstream source has a relative import');
  const start = match.index + match[0].indexOf(match[1]);
  const originalTarget = path.posix.normalize(path.posix.join('src', match[2]));
  const importer = { from: 'src/pool.mjs', to: 'workers/parsing/pool.mjs', source, imports: [{ start, end: start + match[2].length + 2, target: originalTarget }] };
  await writeFile(path.join(root, importer.from), source);
  await executeMoves(root, { moves: [...moves, importer] });
  const result = await readFile(path.join(root, importer.to), 'utf8');
  assert.ok(result.includes(`../../${originalTarget}`));
  assert.equal(result.slice(0, start), source.slice(0, start));
});

test('resumes a persisted transaction with no source inputs after interruption', async t => {
  const { root, moves } = await fixture(t);
  await assert.rejects(executeMoves(root, { moves, onProgress(event) { if (event.phase === 'written') throw new Error('interrupted'); } }), /interrupted/);
  const journal = JSON.parse(await readFile(path.join(root, '.unminify/rename-files/moves.json'), 'utf8'));
  assert.ok(journal.moves.every(move => typeof move.output === 'string' && move.sourceHash && move.outputHash));
  assert.equal((await executeMoves(root, {})).complete, true);
  for (const move of moves) assert.equal(await readFile(path.join(root, move.to), 'utf8'), move.source);
});

test('prevalidates all source hashes and destination ownership before any destructive write', async t => {
  const { root, moves } = await fixture(t);
  await writeFile(path.join(root, moves[1].from), 'externally changed');
  await assert.rejects(executeMoves(root, { moves }), /source changed/);
  assert.equal(await readFile(path.join(root, moves[0].from), 'utf8'), moves[0].source);
});

test('supports cyclic path moves without losing either source', async t => {
  const { root, moves } = await fixture(t);
  moves[0].to = moves[1].from;
  moves[1].to = moves[0].from;
  await executeMoves(root, { moves });
  for (const move of moves) assert.equal(await readFile(path.join(root, move.to), 'utf8'), move.source);
});

test('recovers interruption while clearing cyclic source paths', async t => {
  const { root, moves } = await fixture(t);
  moves[0].to = moves[1].from;
  moves[1].to = moves[0].from;
  await assert.rejects(executeMoves(root, { moves, onProgress(event) { if (event.phase === 'removed') throw new Error('interrupted'); } }), /interrupted/);
  await executeMoves(root, {});
  for (const move of moves) assert.equal(await readFile(path.join(root, move.to), 'utf8'), move.source);
});

test('refuses changed output during resume without writing the remaining outputs', async t => {
  const { root, moves } = await fixture(t);
  await assert.rejects(executeMoves(root, { moves, onProgress(event) { if (event.phase === 'written') throw new Error('interrupted'); } }), /interrupted/);
  await writeFile(path.join(root, moves[0].to), 'external edit');
  await assert.rejects(executeMoves(root, {}), /source changed/);
  await assert.rejects(access(path.join(root, moves[1].to)), { code: 'ENOENT' });
});

test('does not overwrite an existing unselected destination', async t => {
  const { root, moves } = await fixture(t);
  await mkdir(path.join(root, 'animation'));
  await writeFile(path.join(root, moves[1].to), 'not selected');
  await assert.rejects(executeMoves(root, { moves }), /Destination already exists/);
  assert.equal(await readFile(path.join(root, moves[0].from), 'utf8'), moves[0].source);
});

test('completed manifest must still contain the committed output', async t => {
  const { root, moves } = await fixture(t);
  const source = JSON.stringify({ version: 1, chunks: [{ id: 'a', files: ['src/290736.js'] }] });
  await writeFile(path.join(root, 'chunks.json'), source);
  await executeMoves(root, { moves, chunks: 'chunks.json' });
  await writeFile(path.join(root, 'chunks.json'), source);
  await assert.rejects(executeMoves(root, {}), /Chunk manifest changed/);
});

test('moves files with an outside-root chunk manifest and resumes its interrupted update', async t => {
  const { root, moves } = await fixture(t);
  const directory = await mkdtemp(path.join(tmpdir(), 'unminify-external-manifest-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const chunks = path.join(directory, 'chunks.json');
  const member = path.relative(directory, path.join(root, moves[0].from)).split(path.sep).join('/');
  const untouched = path.relative(directory, path.join(root, 'src/ignored.js')).split(path.sep).join('/');
  await writeFile(chunks, JSON.stringify({ version: 1, chunks: [{ id: 'a', files: [member, untouched] }] }));
  await assert.rejects(executeMoves(root, { moves, chunks, onProgress(event) { if (event.phase === 'manifest') throw new Error('interrupted'); } }), /interrupted/);
  assert.equal((await executeMoves(root, { chunks })).complete, true);
  const output = JSON.parse(await readFile(chunks, 'utf8'));
  assert.deepEqual(output.chunks[0].files, [path.relative(directory, path.join(root, moves[0].to)).split(path.sep).join('/'), untouched]);
});

test('outside-root chunk manifests also accept a relative path on creation', async t => {
  const { root, moves } = await fixture(t);
  const directory = await mkdtemp(path.join(tmpdir(), 'unminify-relative-manifest-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const chunks = path.join(directory, 'chunks.json');
  await writeFile(chunks, JSON.stringify({ version: 1, chunks: [{ id: 'a', files: [path.relative(directory, path.join(root, moves[0].from))] }] }));
  assert.equal((await executeMoves(root, { moves, chunks: path.relative(root, chunks) })).complete, true);
});


test('graph and moves preserve real unselected import targets without adding outside-scope edges', async t => {
  const { root } = await fixture(t);
  const from = 'src/checks.mjs', to = 'tools/validation/checks.mjs';
  const source = await readFile(new URL('../scripts/checks.mjs', import.meta.url), 'utf8');
  await writeFile(path.join(root, from), source);
  const dependencies = new Map();
  for (const name of ['timing.mjs', 'identifiers.mjs', 'fragments.mjs']) {
    const original = await readFile(new URL(`../scripts/${name}`, import.meta.url), 'utf8');
    dependencies.set(`src/${name}`, original);
    await writeFile(path.join(root, 'src', name), original);
  }
  const graph = buildDependencyGraph([from], { root, sources: new Map([[from, source]]) });
  assert.deepEqual(graph.edges, []);
  assert.equal(graph.nodes[0].imports.length, 3);
  await executeMoves(root, { moves: [{ from, to, source, imports: graph.nodes[0].imports }] });
  const output = await readFile(path.join(root, to), 'utf8');
  const relocated = buildDependencyGraph([to], { root, sources: new Map([[to, output]]) });
  assert.deepEqual(relocated.edges, []);
  assert.deepEqual(relocated.nodes[0].imports.map(item => item.target), graph.nodes[0].imports.map(item => item.target));
  for (const [file, original] of dependencies) assert.equal(await readFile(path.join(root, file), 'utf8'), original);
});
