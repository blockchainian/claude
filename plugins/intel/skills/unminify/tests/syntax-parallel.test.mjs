import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { transformSyntax } from '../scripts/syntax/runner.mjs';
import { mergeExternalImports } from '../scripts/merge-imports.mjs';
import { authoredSources, parallelFixture } from './helpers/parallel-fixture.mjs';

test('worker syntax errors propagate and closing settles every queued task', { timeout: 10000 }, async () => {
  const { createSyntaxPool } = await import('../scripts/syntax/pool.mjs');
  const pool = createSyntaxPool({ size: 2 });
  try {
    await assert.rejects(pool.run({ kind: 'transform', source: 'export const = ;', options: {} }), /Unexpected token|syntax/i);
    const pending = Array.from({ length: 8 }, () => pool.run({ kind: 'transform', source: 'export const stableFlag = !0;', options: {} }));
    const settled = Promise.allSettled(pending);
    await pool.close();
    assert.equal((await settled).length, pending.length);
  } finally { await pool.close(); }
});

test('real syntax workers match serial transforms and whole-chunk import merges byte for byte', async () => {
  const { createSyntaxPool } = await import('../scripts/syntax/pool.mjs');
  const workers = [], pool = createSyntaxPool({ size: 3, onWorker: event => workers.push(event) });
  try {
    const { files, groups } = authoredSources();
    const results = await Promise.all(Object.values(files).map(source => pool.run({ kind: 'transform', source, options: { fragment: true } })));
    for (const [index, source] of Object.values(files).entries()) {
      const serial = transformSyntax(source, { fragment: true });
      assert.deepEqual(results[index], { source: serial.source, steps: serial.steps.map(({ name, edits }) => ({ name, edits: edits.length })) });
    }
    const inputs = groups.map(group => new Map(group.files.map(file => [file, files[file]])));
    const merged = await Promise.all(inputs.map(sources => pool.run({ kind: 'merge', sources, options: { selected: new Set(sources.keys()), chunkSiblings: true } })));
    for (const [index, sources] of inputs.entries()) {
      assert.deepEqual(merged[index], mergeExternalImports(sources, { selected: new Set(sources.keys()), chunkSiblings: true }));
      assert.notDeepEqual(merged[index], sources, 'the fixture must exercise duplicate-import merging');
    }
    for (const kind of ['transform', 'merge']) assert(new Set(workers.filter(event => event.kind === kind).map(event => event.threadId)).size > 1, `${kind} must use multiple real workers`);
  } finally { await pool.close(); }
});

test('parallel CLI preserves serial source bytes and syntax and chunk journal bytes', async t => {
  const serial = await parallelFixture(t), parallel = await parallelFixture(t), workers = [];
  await serial.serial();
  const result = await parallel.run('restore-syntax', { size: 3, onWorker: event => workers.push(event) });
  assert.equal(result.exitCode, 0);
  assert(new Set(workers.map(event => event.threadId)).size > 1, 'CLI must execute syntax in multiple real workers');
  assert.deepEqual(await parallel.contents(), await serial.contents());
  assert.deepEqual(await parallel.journals(), await serial.journals());
  for (const [file, bytes] of await parallel.contents()) assert.notEqual(bytes.toString(), parallel.files[file], `${file} must exercise a syntax or merge change`);
  assert.equal((await parallel.run()).exitCode, 0);
  assert.deepEqual(await parallel.journals(), await serial.journals());
});

test('SIGKILL after a durable chunk syntax write resumes to uninterrupted source and journal bytes', async t => {
  const uninterrupted = await parallelFixture(t), interrupted = await parallelFixture(t);
  assert.equal((await uninterrupted.run()).exitCode, 0);
  const module = name => new URL(`../scripts/${name}.mjs`, import.meta.url).href;
  const code = `import { run } from ${JSON.stringify(module('cli'))};
    import * as select from ${JSON.stringify(module('select'))};
    import * as syntax from ${JSON.stringify(module('syntax/stage'))};
    await run(${JSON.stringify({ command: 'restore-syntax', paths: [interrupted.root], chunks: interrupted.chunks, minSymbolLength: 1 })},
      { ...select, ...syntax, syntaxPool: { size: 3 }, output(line) {
        const event = JSON.parse(line);
        if (event.chunk && event.status === 'syntax') process.kill(process.pid, 'SIGKILL');
      } });`;
  const child = spawn(process.execPath, ['--input-type=module', '-e', code], { stdio: ['ignore', 'ignore', 'pipe'] });
  const deadline = setTimeout(() => child.kill('SIGKILL'), 30000);
  t.after(() => { clearTimeout(deadline); if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL'); });
  let stderr = '';
  child.stderr.on('data', data => { stderr += data; });
  const [exitCode, signal] = await once(child, 'exit');
  clearTimeout(deadline);
  assert.equal(exitCode, null, stderr);
  assert.equal(signal, 'SIGKILL', stderr);
  const durable = await interrupted.journals();
  assert([...durable].some(([file, bytes]) => file.startsWith('syntax/chunks/') && bytes.toString().includes('"type":"chunk-done"')), 'interrupt only after a chunk commit completed');
  assert.equal((await interrupted.run()).exitCode, 0);
  assert.deepEqual(await interrupted.contents(), await uninterrupted.contents());
  assert.deepEqual(await interrupted.journals(), await uninterrupted.journals());
});

test('rename syntax check uses the same real transform and chunk merge workers without writing source', async t => {
  const fixture = await parallelFixture(t), workers = [];
  let failure;
  try {
    const result = await fixture.run('rename-bindings', { size: 3, onWorker: event => workers.push(event) });
    if (result.exitCode !== 0) failure = fixture.lines.join('\n');
  } catch (error) { failure = error.message; }
  assert.match(failure ?? '', /restore-syntax/);
  for (const file of Object.keys(fixture.files)) assert(failure.includes(file), `check-syntax must include ${file}`);
  for (const kind of ['transform', 'merge']) assert(new Set(workers.filter(event => event.kind === kind).map(event => event.threadId)).size > 1, `check-syntax ${kind} must use multiple real workers`);
  for (const [file, bytes] of await fixture.contents()) assert.equal(bytes.toString(), fixture.files[file]);
});
