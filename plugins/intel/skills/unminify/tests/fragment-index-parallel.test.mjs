import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, mkdir, symlink, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { indexFragmentContext, loadFragmentContext } from '../scripts/fragments.mjs';
import { createSyntaxPool } from '../scripts/syntax/pool.mjs';
import { parallelFixture } from './helpers/parallel-fixture.mjs';
import { fragmentSources } from './helpers/fragment-fixture.mjs';

// assert.deepEqual on Set/Map alone does not compare insertion order.
const ordered = context => ({ fragments: [...context.fragments], topLevelBindings: [...context.topLevelBindings] });

test('fragment index uses multiple real workers and matches serial context including insertion order', async t => {
  const sources = fragmentSources();
  sources.files['jsx.js'] = 'const view = <section>{sharedAlias}</section>;\n';
  const f = await parallelFixture(t, sources), workers = [];
  await mkdir(join(f.root, 'directory.js'));
  await symlink(join(f.root, 'declaration.js'), join(f.root, 'link.js'));
  await writeFile(join(f.root, 'ignored.js'), 'const ignoredBinding = 1;\n');
  await writeFile(join(f.root, '.gitignore'), 'ignored.js\n');
  // Include a tracked path that has disappeared, plus duplicates and ignored explicit input.
  await writeFile(join(f.root, 'deleted.js'), 'const deletedBinding = 1;\n');
  execFileSync('git', ['add', '--', 'deleted.js'], { cwd: f.root });
  const { unlink } = await import('node:fs/promises');
  await unlink(join(f.root, 'deleted.js'));
  const inputs = ['standalone-1.js', 'ignored.js', 'standalone-1.js', 'missing.js'];
  const files = execFileSync('git', ['ls-files', '-z', '--cached', '--others', '--exclude-standard'], { cwd: f.root, encoding: 'utf8' }).split('\0').filter(Boolean);
  const serialSources = [...new Set([...files, ...inputs])].filter(file => Object.hasOwn(sources.files, file) || file === 'ignored.js')
    .map(file => [file, file === 'ignored.js' ? 'const ignoredBinding = 1;\n' : sources.files[file]]);
  const expected = indexFragmentContext(serialSources);
  assert(expected.fragments.has('alias.js'));
  assert(expected.fragments.has('declaration.js'));
  assert(!expected.fragments.has('invalid.js'));
  assert(!expected.fragments.has('ordinary.cjs'));
  assert(expected.topLevelBindings.has('undefined'));
  const pool = createSyntaxPool({ size: 3, onWorker: event => workers.push(event) });
  try {
    const parallel = await loadFragmentContext(f.root, inputs, pool);
    assert.deepEqual(ordered(parallel), ordered(expected));
    assert.deepEqual(parallel, loadFragmentContext(f.root, inputs));
    assert(new Set(workers.filter(event => event.kind === 'fragment').map(event => event.threadId)).size > 1,
      'fragment indexing must use multiple real syntax workers');
  } finally { await pool.close(); }
});

test('real restore-syntax matches captured 0.71.3 source and journal bytes and resumes unchanged', async t => {
  const baseline = JSON.parse(await readFile(new URL('./fixtures/fragment-restore-baseline.json', import.meta.url), 'utf8'));
  const f = await parallelFixture(t, fragmentSources()), workers = [];
  const summary = await f.run('restore-syntax', { size: 3, onWorker: event => workers.push(event) });
  assert.equal(summary.exitCode, 0);
  assert(new Set(workers.filter(event => event.kind === 'fragment').map(event => event.threadId)).size > 1,
    'the CLI must use the shared pool for fragment indexing');
  assert.deepEqual(Object.keys(summary.phase_ms).sort(), baseline.phaseKeys);
  const expectedFiles = new Map(baseline.files.map(([file, source]) => [file, Buffer.from(source)]));
  const expectedJournals = new Map(baseline.journals.map(([file, source]) => [file, Buffer.from(source)]));
  assert.deepEqual(await f.contents(), expectedFiles);
  assert.deepEqual(await f.journals(), expectedJournals);
  assert.equal((await f.run()).exitCode, 0);
  assert.deepEqual(await f.contents(), expectedFiles);
  assert.deepEqual(await f.journals(), expectedJournals);
});

test('fragment workers read paths and return plain per-file facts without source or syntax trees', async t => {
  const f = await parallelFixture(t, fragmentSources());
  const pool = createSyntaxPool({ size: 2 });
  try {
    const summary = await pool.run({ kind: 'fragment', path: join(f.root, 'alias.js') });
    assert.deepEqual(summary, {
      topLevelBindings: ['sharedAlias'], declarations: [], aliases: ['sharedAlias'], free: [],
      facts: { fragment: false, unboundExports: [], topLevelNames: ['sharedAlias'], unknown: [], unused: ['sharedAlias'], boundary: false },
    });
    for (const file of ['invalid.js', 'missing.js']) assert.equal(await pool.run({ kind: 'fragment', path: join(f.root, file) }), null);
    await mkdir(join(f.root, 'folder.js'));
    await symlink(join(f.root, 'alias.js'), join(f.root, 'symbolic.js'));
    for (const file of ['folder.js', 'symbolic.js']) assert.equal(await pool.run({ kind: 'fragment', path: join(f.root, file) }), null);
  } finally { await pool.close(); }
});

test('fragment task queue is bounded and no-Git fallback preserves input order', async t => {
  const { availableParallelism } = await import('node:os');
  const { rm } = await import('node:fs/promises');
  const sources = fragmentSources(), f = await parallelFixture(t, sources);
  await rm(join(f.root, '.git'), { recursive: true });
  const inputs = Object.keys(sources.files).reverse();
  const expected = indexFragmentContext(inputs.map(file => [file, sources.files[file]]));
  const pool = createSyntaxPool({ size: 3 });
  let active = 0, peak = 0, calls = 0;
  const observed = { run(task) {
    assert.deepEqual(Object.keys(task).sort(), ['kind', 'path']);
    assert.equal(task.kind, 'fragment');
    calls++;
    peak = Math.max(peak, ++active);
    return pool.run(task).finally(() => { active--; });
  } };
  try {
    assert.deepEqual(ordered(await loadFragmentContext(f.root, inputs, observed)), ordered(expected));
    assert.equal(calls, inputs.length);
    assert(peak > 1 && peak <= availableParallelism(), `queued tasks must be bounded: ${peak}`);
    assert.equal(active, 0);
    await pool.close();
    await assert.rejects(loadFragmentContext(f.root, inputs, pool), /syntax pool is closed/);
  } finally { await pool.close(); }
});
