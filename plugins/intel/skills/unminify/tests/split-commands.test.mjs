import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { run, parseArgs } from '../scripts/cli.mjs';
import * as select from '../scripts/select.mjs';
import * as rename from '../scripts/rename.mjs';
import * as checks from '../scripts/checks.mjs';
import * as syntax from '../scripts/syntax/stage.mjs';
import { createNamer } from '../scripts/model.mjs';
import { modelService } from './helpers/model-service.mjs';

// Captured by executing the pre-split run on these authored sources, with no model requests.
const baseline = JSON.parse(await readFile(new URL('./helpers/split-baseline.json', import.meta.url), 'utf8'));
const commands = ['restore-syntax', 'rename-bindings', 'sync-imports'];
const cli = fileURLToPath(new URL('../scripts/cli.mjs', import.meta.url));

async function fixture(t, files = baseline.files, groups = baseline.groups) {
  const root = await mkdtemp(join(tmpdir(), 'unminify-split-'));
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  for (const [file, source] of Object.entries(files)) {
    await mkdir(join(root, file, '..'), { recursive: true });
    await writeFile(join(root, file), source);
  }
  const chunks = join(root, 'chunks.json');
  await writeFile(chunks, JSON.stringify({ version: 1, chunks: groups }));
  t.after(() => rm(root, { recursive: true, force: true }));
  let requests = 0;
  const transport = await modelService(t, root, (_req, res) => {
    requests++;
    // A forbidden-request detector; no model output or naming behavior is simulated.
    res.writeHead(400); res.end('{}');
  });
  const lines = [];
  return { root, chunks, lines, get requests() { return requests; },
    read: file => readFile(join(root, file), 'utf8'),
    async contents() { return Object.fromEntries(await Promise.all(Object.keys(files).map(async file => [file, await this.read(file)]))); },
    run(command, paths = [root], settings = {}) {
      return run({ command, paths, chunks, minSymbolLength: 1, ...settings }, {
        ...select, ...rename, ...checks, ...syntax,
        createNamer: options => createNamer({ ...options, ...transport, retryDelayMs: 1,
          refreshUrl: `${transport.baseUrl}/oauth/token` }),
        output: line => lines.push(line),
      });
    },
  };
}

for (const command of commands) {
  test(`${command} parses its command and shared options`, () => {
    const options = parseArgs([command, 'src', '--ignore', 'omit.txt', '--chunks', 'chunks.json']);
    assert.equal(options.command, command);
    assert.deepEqual(options.paths, [join(process.cwd(), 'src')]);
    assert.equal(options.ignoreFile, join(process.cwd(), 'omit.txt'));
    assert.equal(options.chunks, join(process.cwd(), 'chunks.json'));
  });
}

for (const [command, forbidden] of [
  ['restore-syntax', ['--min-symbol-length', '--concurrency', '--max-input-tokens']],
  ['rename-bindings', ['--model', '--max-ids', '--chunk-siblings']],
  ['sync-imports', ['--concurrency', '--max-input-tokens', '--model']],
]) {
  test(`${command} rejects every parameter outside its phase`, () => {
    for (const option of forbidden) {
      assert.throws(() => parseArgs([command, 'src', option, '2']),
        new RegExp(`invalid option: ${option}`));
    }
  });
}

test('rename-bindings and sync-imports accept only their documented numeric options', () => {
  const options = parseArgs(['rename-bindings', 'src', '--min-symbol-length', '3', '--concurrency', '2', '--max-input-tokens', '500']);
  assert.equal(options.minSymbolLength, 3);
  assert.equal(options.concurrency, 2);
  assert.equal(options.maxInputTokens, 500);
  assert.equal(parseArgs(['sync-imports', 'src', '--min-symbol-length', '3']).minSymbolLength, 3);
});

test('missing, unknown and removed path-first commands print usage and fail', () => {
  for (const args of [[], ['src'], ['unknown', 'src']]) {
    const result = spawnSync(process.execPath, [cli, ...args], { encoding: 'utf8' });
    assert.ifError(result.error);
    assert.notEqual(result.status, 0);
    const output = result.stdout + result.stderr;
    for (const command of commands) assert.match(output, new RegExp(command));
    assert.throws(() => parseArgs(args));
  }
});

test('rename-bindings lists every unrestored selected file and chunk sibling before any request', async t => {
  const files = { 'selected.js': 'const a = !0; console.log(a);\n',
    'first.js': 'const b = !1;\n', 'sibling.js': 'console.log(b, void 0);\n' };
  const f = await fixture(t, files, [{ id: 'split', files: ['first.js', 'sibling.js'] }]);
  let failure;
  try {
    const result = await f.run('rename-bindings', [join(f.root, 'selected.js'), join(f.root, 'first.js')], { minSymbolLength: 4 });
    if (result.exitCode !== 0) failure = f.lines.join('\n');
  } catch (error) { failure = error.message; }
  assert(failure, 'unrestored syntax must reject the naming phase');
  assert.match(failure, /restore-syntax/);
  for (const file of Object.keys(files)) assert.match(failure, new RegExp(file.replace('.', '\\.')));
  assert.equal(f.requests, 0);
  assert.deepEqual(await f.contents(), files);
});

test('restore-syntax expands chunk siblings, filters ignored and invalid files, and resumes without naming or imports', async t => {
  const files = { 'first.js': 'import { basename as a } from "node:path"; const flag = !0;\n',
    'sibling.js': 'console.log(a, flag, void 0);\n', 'ignored.js': 'const ignored = !0;\n', 'invalid.js': 'const = ;\n' };
  const f = await fixture(t, files, [{ id: 'shared', files: ['first.js', 'sibling.js'] }]);
  await writeFile(join(f.root, '.ignore'), 'ignored.js\n');
  const result = await f.run('restore-syntax', [join(f.root, 'first.js'), join(f.root, 'ignored.js'), join(f.root, 'invalid.js')]);
  assert.equal(result.exitCode, 0);
  assert.match(await f.read('first.js'), /flag = true/);
  assert.match(await f.read('first.js'), /basename as a/);
  assert.match(await f.read('sibling.js'), /undefined/);
  assert.equal(await f.read('ignored.js'), files['ignored.js']);
  assert.equal(await f.read('invalid.js'), files['invalid.js']);
  const after = await f.contents();
  assert.equal((await f.run('restore-syntax', [join(f.root, 'first.js')])).exitCode, 0);
  assert.deepEqual(await f.contents(), after);
  assert.equal(f.requests, 0);
});

test('three separate phases match the actual pre-split run byte for byte and each resumes', async t => {
  const f = await fixture(t);
  assert.equal((await f.run('restore-syntax')).exitCode, 0);
  const restored = await f.contents();
  assert.match(restored['src/a/exporter.js'], /readFileSync as a/);
  assert.equal((await f.run('rename-bindings')).exitCode, 0);
  assert.deepEqual(await f.contents(), restored, 'naming must not follow external imports');
  assert.equal((await f.run('sync-imports')).exitCode, 0);
  assert.deepEqual(await f.contents(), baseline.expected);
  for (const command of commands) assert.equal((await f.run(command)).exitCode, 0);
  assert.deepEqual(await f.contents(), baseline.expected);
  assert.equal(f.requests, 0);
});

test('module batches in either dependency order match whole-directory phases byte for byte', async t => {
  const whole = await fixture(t);
  for (const command of commands) assert.equal((await whole.run(command)).exitCode, 0);
  const expected = await whole.contents();
  for (const order of [['a', 'b'], ['b', 'a']]) {
    const f = await fixture(t);
    for (const module of order) {
      for (const command of commands) assert.equal((await f.run(command, [join(f.root, 'src', module)])).exitCode, 0);
    }
    assert.deepEqual(await f.contents(), expected, `module order ${order}`);
    assert.equal(f.requests, 0);
  }
});

test('sync-imports updates previously named files beyond its selected path', async t => {
  const f = await fixture(t);
  for (const command of ['restore-syntax', 'rename-bindings']) assert.equal((await f.run(command)).exitCode, 0);
  assert.equal((await f.run('sync-imports', [join(f.root, 'src/b/consumer.js')])).exitCode, 0);
  assert.deepEqual(await f.contents(), baseline.expected);
  assert.equal(f.requests, 0);
});

test('sync-imports expands newly selected chunk siblings without restoring syntax or naming', async t => {
  const files = { 'first.js': 'import { basename as a } from "node:path"; const flag = !0;\n',
    'sibling.js': 'console.log(a, flag); export { a as filename };\n' };
  const f = await fixture(t, files, [{ id: 'shared', files: Object.keys(files) }]);
  assert.equal((await f.run('sync-imports', [join(f.root, 'first.js')])).exitCode, 0);
  assert.equal(await f.read('first.js'), 'import { basename } from "node:path"; const flag = !0;\n');
  assert.equal(await f.read('sibling.js'), 'console.log(basename, flag); export { basename as filename };\n');
  assert.equal(f.requests, 0);
  const after = await f.contents();
  assert.equal((await f.run('sync-imports', [join(f.root, 'first.js')])).exitCode, 0);
  assert.deepEqual(await f.contents(), after);
});


test('real CLI executes the three commands separately without credentials or model requests', async t => {
  const f = await fixture(t);
  for (const command of commands) {
    const args = [cli, command, f.root, '--chunks', f.chunks];
    if (command !== 'restore-syntax') args.push('--min-symbol-length', '1');
    const result = spawnSync(process.execPath, args, {
      encoding: 'utf8', timeout: 30000,
      env: { ...process.env, CODEX_HOME: join(f.root, 'absent-auth') },
    });
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stderr);
    const summary = result.stdout.trim().split('\n').map(JSON.parse).findLast(record => record.type === 'summary');
    assert.equal(summary.requests, 0);
  }
  assert.deepEqual(await f.contents(), baseline.expected);
});


test('sample runner executes all three CLI stages for an authored credential-free sample', async t => {
  const f = await fixture(t, { 'sample.js': 'const stableFlag = !0; console.log(stableFlag);\n' }, []);
  const result = spawnSync(process.execPath, [fileURLToPath(new URL('../scripts/e2e.mjs', import.meta.url)), join(f.root, 'sample.js')], {
    encoding: 'utf8', timeout: 30000,
    env: { ...process.env, CODEX_HOME: join(f.root, 'absent-auth') },
  });
  const temporaryRepo = /^Temporary repository: (.+)$/m.exec(result.stdout)?.[1];
  if (temporaryRepo) t.after(() => rm(join(temporaryRepo, '..'), { recursive: true, force: true }));
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /const stableFlag = true; console.log\(stableFlag\)/);
  const summaries = result.stdout.split('\n').filter(line => line.startsWith('{')).map(JSON.parse).filter(record => record.type === 'summary');
  assert.equal(summaries.length, 3);
  assert(summaries.every(summary => summary.requests === 0));
});
