import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { run } from '../scripts/cli.mjs';
import * as select from '../scripts/select.mjs';
import * as syntax from '../scripts/syntax/stage.mjs';
import { createSyntaxPool } from '../scripts/syntax/pool.mjs';

// Authored modules only; no recovered application code.
async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), 'unminify-selection-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  const files = {};
  for (let index = 0; index < 8; index++) files[`src/file-${index}.js`] =
    `import { basename as first } from 'node:path';\nimport { basename as second } from 'node:path';\nexport const value${index} = first('a') + second('b');\n`;
  Object.assign(files, { 'chunk/first.js': 'const sharedFlag = !0;\n',
    'chunk/last.js': 'console.log(sharedFlag);\n', 'chunk/ignored.js': 'const ignoredFlag = !0;\n',
    'invalid.js': 'const = ;\n' });
  for (const [file, source] of Object.entries(files)) {
    await mkdir(join(root, file, '..'), { recursive: true });
    await writeFile(join(root, file), source);
  }
  await writeFile(join(root, '.ignore'), 'chunk/ignored.js\n');
  const chunks = join(root, 'chunks.json');
  await writeFile(chunks, JSON.stringify({ version: 1, chunks: [{ id: 'shared',
    files: ['chunk/first.js', 'chunk/last.js', 'chunk/ignored.js'] }] }));
  const workers = [], calls = [], lines = [];
  const paths = [join(root, 'src'), join(root, 'chunk/first.js'), join(root, 'invalid.js')];
  return { root, workers, calls, files, async execute() {
    return run({ command: 'restore-syntax', paths, chunks }, { ...select, ...syntax,
      async listFiles(target, options) {
        const result = await select.listFiles(target, options);
        calls.push({ target, result });
        return result;
      },
      syntaxPool: { size: 3, onWorker: event => workers.push(event) }, output: line => lines.push(JSON.parse(line)),
    });
  } };
}

test('selection uses multiple real parse workers and preserves the serial file lists and ignored sibling', async t => {
  const f = await fixture(t);
  const summary = await f.execute();
  assert.equal(summary.exitCode, 0);
  // Captured on 0.71.2 from this exact fixture, before changing selection.
  const expected = Array.from({ length: 8 }, (_, i) => `src/file-${i}.js`).concat('chunk/first.js');
  const initial = f.calls.slice(0, 3).flatMap(call => call.result);
  const expanded = [...new Set(f.calls.flatMap(call => call.result))].sort();
  assert.deepEqual(initial, expected);
  assert.deepEqual(expanded, ['chunk/first.js', 'chunk/last.js', ...expected.slice(0, 8)]);
  assert.equal(summary.files, expected.length);
  assert.equal(await readFile(join(f.root, 'chunk/ignored.js'), 'utf8'), f.files['chunk/ignored.js']);
  assert(new Set(f.workers.filter(event => event.kind === 'parse').map(event => event.threadId)).size > 1);
  assert.equal(f.calls.filter(call => Array.isArray(call.target)).length, 1, 'expand siblings in one call');
});

test('whole CLI run resolves Git root exactly once through real git subprocesses', async t => {
  const f = await fixture(t);
  const realGit = execFileSync('which', ['git'], { encoding: 'utf8' }).trim();
  const bin = join(f.root, 'bin'), counter = join(f.root, 'git-calls');
  await mkdir(bin);
  const quote = value => `'${value.replaceAll("'", "'\\''")}'`;
  await writeFile(join(bin, 'git'), `#!/bin/sh\nif [ "$1" = rev-parse ]; then printf '%s\\n' "$*" >> ${quote(counter)}; fi\nexec ${quote(realGit)} "$@"\n`, { mode: 0o755 });
  const originalPath = process.env.PATH;
  process.env.PATH = `${bin}:${originalPath}`;
  try { assert.equal((await f.execute()).exitCode, 0); }
  finally { process.env.PATH = originalPath; }
  assert.deepEqual((await readFile(counter, 'utf8')).trim().split('\n'), ['rev-parse --show-toplevel']);
});

test('ordinary import merging has its own summary phase outside selection_plan', async t => {
  const f = await fixture(t), summary = await f.execute();
  const events = (await readFile(join(f.root, '.unminify/restore-syntax/performance.jsonl'), 'utf8')).trim().split('\n').map(JSON.parse);
  assert.equal(summary.exitCode, 0);
  assert.equal(typeof summary.phase_ms.merge_imports, 'number');
  assert(summary.phase_ms.merge_imports > 0);
  const selections = events.filter(event => event.type === 'phase_start' && event.phase === 'selection_plan');
  assert.equal(selections.length, 1, 'restore-syntax selection phase excludes every import merge');
  const end = events.find(event => event.type === 'phase_end' && event.phase === 'selection_plan');
  const merges = events.filter(event => event.type === 'phase_start' && event.phase === 'merge_imports');
  assert.equal(merges.length, 8);
  assert(merges.every(event => event.at >= end.at));
});

test('parse workers return only parseability and parse time, and callbacks exclude invalid files', async t => {
  const f = await fixture(t), pool = createSyntaxPool({ size: 2 });
  try {
    for (const [source, parseable] of [['export { missing };', true], ['const = ;', false]]) {
      const result = await pool.run({ kind: 'parse', source });
      assert.deepEqual(Object.keys(result).sort(), ['elapsed', 'parseable']);
      assert.equal(result.parseable, parseable);
      assert(result.elapsed > 0);
    }
    const parsed = [];
    const selected = await select.listFiles(f.root, { pool, onParse: (...args) => parsed.push(args) });
    assert.deepEqual(parsed.map(([file]) => file).sort(), selected);
    assert(parsed.every(([, at, elapsed]) => at > 0 && elapsed > 0));
    assert(!selected.includes('invalid.js'));
    assert(!selected.includes('chunk/ignored.js'));
  } finally { await pool.close(); }
});
