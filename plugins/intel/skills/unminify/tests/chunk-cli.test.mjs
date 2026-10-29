import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync, spawn } from 'node:child_process';
import { once } from 'node:events';
import { run, parseArgs } from '../scripts/cli.mjs';
import * as select from '../scripts/select.mjs';
import * as rename from '../scripts/rename.mjs';
import * as checks from '../scripts/checks.mjs';
import * as syntax from '../scripts/syntax/stage.mjs';
import { createNamer } from '../scripts/model.mjs';
import { modelService, complete } from './helpers/model-service.mjs';

async function fixture(t, files, groups = [{ id: 'one', files: Object.keys(files) }], controls = {}) {
  const root = await mkdtemp(join(tmpdir(), 'unminify-chunk-cli-'));
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  for (const [file, source] of Object.entries(files)) {
    await mkdir(join(root, file, '..'), { recursive: true });
    await writeFile(join(root, file), source);
  }
  const chunks = join(root, 'chunks.json');
  await writeFile(chunks, JSON.stringify({ version: 1, chunks: groups }));
  t.after(() => rm(root, { recursive: true, force: true }));
  const requests = [], lines = [];
  const service = await modelService(t, root, async (req, res) => {
    let raw = ''; for await (const part of req) raw += part;
    const body = JSON.parse(raw), context = body.input[0].content[0].text;
    requests.push({ body, context });
    if (await controls.handle?.({ req, res, context, requests })) return;
    const names = Object.fromEntries(body.text.format.schema.required.map(key => [key,
      new RegExp(`const ${key} = 11`).test(context) ? 'readableValue' : 'argumentValue']));
    complete(res, names, requests.length);
  });
  return { root, chunks, requests, lines, service, controls,
    read: file => readFile(join(root, file), 'utf8'),
    run: settings => run({ paths: [root], chunks, concurrency: 2, ...settings }, {
      ...select, ...rename, ...checks, ...syntax,
      createNamer: options => createNamer({ ...options, ...service, retryDelayMs: 1 }),
      output: line => lines.push(line),
    }),
  };
}
const slices = { 'a.js': 'const a = 11;\n', 'b.js': 'console.log(a); export { a };\n' };

test('chunk CLI arguments accept a manifest and reject the removed sibling flag', () => {
  const options = parseArgs(['rename-bindings', 'src', '--chunks', 'chunks.json']);
  assert.equal(options.chunks, join(process.cwd(), 'chunks.json'));
  assert.throws(() => parseArgs(['rename-bindings', 'src', '--chunks', 'chunks.json', '--chunk-siblings']), /invalid option: --chunk-siblings/);
});
test('chunk naming asks once, renders sibling use, preserves export and replays completion', async t => {
  const f = await fixture(t, slices);
  const result = await f.run();
  assert.equal(result.exitCode, 0);
  assert.equal(await f.read('a.js'), 'const readableValue = 11;\n');
  assert.equal(await f.read('b.js'), 'console.log(readableValue); export { readableValue as a };\n');
  assert.equal(f.requests.length, 1);
  assert.match(f.requests[0].context, /console\.log\(v\d+\)/);
  assert.equal((await f.run()).exitCode, 0);
  assert.equal(f.requests.length, 1);
});
test('chunk selection expands to siblings', async t => {
  const f = await fixture(t, slices);
  const result = await f.run({ paths: [join(f.root, 'a.js')] });
  assert.equal(result.exitCode, 0);
  assert.equal(f.requests.length, 1);
  assert.equal(await f.read('a.js'), 'const readableValue = 11;\n');
  assert.equal(await f.read('b.js'), 'console.log(readableValue); export { readableValue as a };\n');
});
test('chunk duplicate declarations freeze without merging unrelated same-name chunks', async t => {
  const files = { ...slices, 'c.js': 'const a = 12;\n', 'd.js': 'console.log(a);\n' };
  const groups = [{ id: 'one', files: ['a.js', 'b.js'] }, { id: 'two', files: ['c.js', 'd.js'] }];
  const f = await fixture(t, files, groups);
  assert.equal((await f.run()).exitCode, 0);
  assert.match(await f.read('a.js'), /readableValue/);
  assert.match(await f.read('c.js'), /argumentValue/);
  const duplicate = await fixture(t, { 'a.js': 'const a = 11;\n', 'b.js': 'const a = 12; console.log(a);\n' });
  assert.equal((await duplicate.run()).exitCode, 0);
  assert.equal(duplicate.requests.length, 0);
  assert.equal(await duplicate.read('a.js'), 'const a = 11;\n');
});
test('ignored sibling references freeze even with expansion', async t => {
  const files = { 'a.js': slices['a.js'], 'vendor/b.js': slices['b.js'] };
  const f = await fixture(t, files);
  await writeFile(join(f.root, '.ignore'), 'vendor/\n');
  assert.equal((await f.run()).exitCode, 0);
  assert.equal(f.requests.length, 0);
  for (const [file, source] of Object.entries(files)) assert.equal(await f.read(file), source);
});
test('chunk import aliases follow an exporter without a model request and update sibling uses', async t => {
  const files = { 'remote.js': 'const a = 11; export { a };\n',
    'first.js': 'import { a as z } from "./remote.js";\n', 'last.js': 'console.log(z); export { z };\n' };
  const f = await fixture(t, files, [{ id: 'consumer', files: ['first.js', 'last.js'] }]);
  assert.equal((await f.run()).exitCode, 0);
  assert.match(await f.read('remote.js'), /readableValue as a/);
  assert.equal(await f.read('first.js'), 'import { a as readableValue } from "./remote.js";\n');
  assert.equal(await f.read('last.js'), 'console.log(readableValue); export { readableValue as z };\n');
  assert.equal(f.requests.length, 1);
  assert.equal((await f.run()).exitCode, 0);
  assert.equal(f.requests.length, 1);
});
test('a real syntax gate rejects the entire chunk including pending syntax changes', async t => {
  const files = { 'a.js': 'const a = !0;\n', 'b.js': 'function collect(x, x) { return a + x; } console.log(collect(1, 2));\n' };
  const f = await fixture(t, files);
  const result = await f.run();
  assert.equal(result.exitCode, 1);
  assert(result.rejected > 0);
  for (const [file, source] of Object.entries(files)) assert.equal(await f.read(file), source);
});
test('saved chunk top-level replies survive SIGKILL and resume identically', async t => {
  const files = { 'a.js': 'const a = 11;\n', 'b.js': 'function calculate(b) { return a + b; } export { a };\n' };
  let child;
  const f = await fixture(t, files, undefined, { handle: ({ context }) => {
    if (/calculate\(v\d+\)/.test(context)) { child.kill('SIGKILL'); return true; }
  } });
  const imports = name => new URL(`../scripts/${name}.mjs`, import.meta.url).href;
  const script = `import {run} from ${JSON.stringify(imports('cli'))};
    import * as select from ${JSON.stringify(imports('select'))};
    import * as rename from ${JSON.stringify(imports('rename'))};
    import * as checks from ${JSON.stringify(imports('checks'))};
    import * as syntax from ${JSON.stringify(imports('syntax/stage'))};
    import {createNamer} from ${JSON.stringify(imports('model'))};
    await run(${JSON.stringify({ paths: [f.root], chunks: f.chunks, concurrency: 1 })}, {...select,...rename,...checks,...syntax,
      createNamer: options => createNamer({...options,...${JSON.stringify(f.service)}})});`;
  child = spawn(process.execPath, ['--input-type=module', '-e', script], { stdio: ['ignore', 'ignore', 'pipe'] });
  t.after(() => { if (child.exitCode === null) child.kill('SIGKILL'); });
  let stderr = ''; child.stderr.on('data', data => { stderr += data; });
  const [, signal] = await once(child, 'exit');
  assert.equal(signal, 'SIGKILL', stderr);
  for (const [file, source] of Object.entries(files)) assert.equal(await f.read(file), source);
  f.controls.handle = undefined;
  assert.equal((await f.run()).exitCode, 0);
  const uninterrupted = await fixture(t, files);
  assert.equal((await uninterrupted.run()).exitCode, 0);
  for (const file of Object.keys(files)) assert.equal(await f.read(file), await uninterrupted.read(file));
  assert.equal(f.requests.length, uninterrupted.requests.length + 1);
});
test('without --chunks the legacy fragment protection remains in force', async t => {
  const f = await fixture(t, slices);
  assert.equal((await f.run({ chunks: undefined })).exitCode, 0);
  for (const [file, source] of Object.entries(slices)) assert.equal(await f.read(file), source);
  assert.equal(f.requests.length, 0);
});

test('a frozen duplicate import alias does not abort import following', async t => {
  const files = { 'remote.js': 'const longValue = 11; export { longValue as value };\n',
    'a.js': 'import { value as z } from "./remote.js";\n',
    'b.js': 'import { value as z } from "./remote.js"; console.log(z);\n' };
  const f = await fixture(t, files, [{ id: 'consumer', files: ['a.js', 'b.js'] }]);
  assert.equal((await f.run({ minSymbolLength: 1 })).exitCode, 0);
  for (const [file, source] of Object.entries(files)) assert.equal(await f.read(file), source);
  assert.equal(f.requests.length, 0);
});
test('chunk import aliases stay unchanged when their exporting binding did not change', async t => {
  const files = { 'remote.js': 'const a = 11; export { a };\n',
    'first.js': 'import { a as z } from "./remote.js";\n', 'last.js': 'console.log(z);\n' };
  const f = await fixture(t, files, [{ id: 'consumer', files: ['first.js', 'last.js'] }]);
  assert.equal((await f.run({ minSymbolLength: 1 })).exitCode, 0);
  for (const [file, source] of Object.entries(files)) assert.equal(await f.read(file), source);
  assert.equal(f.requests.length, 0);
});
test('dynamic chunk bindings and locals remain unchanged without model requests', async t => {
  const files = { 'a.js': 'const a = 11;\n', 'b.js': 'function calculate(b) { return eval("a + b"); } console.log(calculate(2));\n' };
  const f = await fixture(t, files);
  assert.equal((await f.run()).exitCode, 0);
  for (const [file, source] of Object.entries(files)) assert.equal(await f.read(file), source);
  assert.equal(f.requests.length, 0);
});

test('chunk syntax is committed before naming while import following waits for validation', async t => {
  const files = { 'remote.js': 'const a = 11; export { a };\n',
    'first.js': 'import { a as z } from "./remote.js"; const stableValue = !0;\n', 'last.js': 'console.log(z);\n' };
  let observed;
  const f = await fixture(t, files, [{ id: 'consumer', files: ['first.js', 'last.js'] }], { handle: async () => {
    observed = await f.read('first.js');
    return false;
  } });
  assert.equal((await f.run({ concurrency: 1 })).exitCode, 0);
  assert.equal(observed, files['first.js'].replace('!0', 'true'));
  assert.match(await f.read('first.js'), /readableValue/);
  assert.match(await f.read('first.js'), /stableValue = true/);
});

test('duplicate import aliases remain frozen when the exporter really changes', async t => {
  const files = { 'remote.js': 'const a = 11; export { a };\n',
    'first.js': 'import { a as z } from "./remote.js";\n',
    'last.js': 'import { a as z } from "./remote.js"; console.log(z);\n' };
  const f = await fixture(t, files, [{ id: 'consumer', files: ['first.js', 'last.js'] }]);
  assert.equal((await f.run()).exitCode, 0);
  assert.match(await f.read('remote.js'), /readableValue/);
  for (const file of ['first.js', 'last.js']) assert.equal(await f.read(file), files[file]);
  assert.equal(f.requests.length, 1);
});
test('outside chunk import declarations follow in sibling chunk members', async t => {
  const files = { 'remote.js': 'const a = 11; export { a };\n',
    'first.js': 'import { a as z } from "./remote.js";\n', 'last.js': 'console.log(z);\n' };
  const f = await fixture(t, files, [{ id: 'consumer', files: ['first.js', 'last.js'] }]);
  assert.equal((await f.run({ paths: [join(f.root, 'remote.js'), join(f.root, 'last.js')] })).exitCode, 0);
  assert.equal(await f.read('first.js'), 'import { a as readableValue } from "./remote.js";\n');
  assert.equal(await f.read('last.js'), 'console.log(readableValue);\n');
});
test('chunk exports declared in a sibling propagate through another chunk to a standalone importer', async t => {
  const files = { 'decl.js': 'const a = 11;\n', 'export.js': 'export { a };\n',
    'first.js': 'import { a as z } from "./export.js";\n', 'last.js': 'console.log(z); export { z };\n',
    'downstream.js': 'import { z as k } from "./last.js"; console.log(k);\n' };
  const f = await fixture(t, files, [{ id: 'producer', files: ['decl.js', 'export.js'] }, { id: 'consumer', files: ['first.js', 'last.js'] }]);
  assert.equal((await f.run()).exitCode, 0);
  assert.match(await f.read('export.js'), /readableValue as a/);
  assert.match(await f.read('last.js'), /readableValue as z/);
  assert.match(await f.read('first.js'), /a as readableValue/);
  assert.match(await f.read('downstream.js'), /z as readableValue/);
  assert.equal(f.requests.length, 1);
  assert.equal((await f.run()).exitCode, 0);
  assert.equal(f.requests.length, 1);
});
test('CLI completes a real prepared half-write before rebuilding chunk plans', async t => {
  const f = await fixture(t, slices);
  assert.equal((await f.run()).exitCode, 0);
  const expected = new Map(await Promise.all(Object.keys(slices).map(async file => [file, await f.read(file)])));
  const { readdir } = await import('node:fs/promises');
  const directory = join(f.root, '.unminify/rename-bindings/chunks');
  const [name] = await readdir(directory), path = join(directory, name);
  const records = (await readFile(path, 'utf8')).trim().split('\n').map(line => JSON.parse(line));
  const prepare = records.findLastIndex(record => record.type === 'chunk-prepare');
  assert(prepare > 0);
  await writeFile(path, records.slice(0, prepare + 1).map(record => JSON.stringify(record)).join('\n') + '\n');
  await writeFile(join(f.root, 'b.js'), slices['b.js']);
  assert.equal((await f.run()).exitCode, 0);
  for (const [file, source] of expected) assert.equal(await f.read(file), source);
  assert.equal(f.requests.length, 1);
});
