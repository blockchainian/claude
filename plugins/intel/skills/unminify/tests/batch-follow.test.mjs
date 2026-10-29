import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, writeFile, readdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { run } from '../scripts/cli.mjs';
import { parseRunArgs } from './helpers/run-stages.mjs';
import * as select from '../scripts/select.mjs';
import * as rename from '../scripts/rename.mjs';
import * as checks from '../scripts/checks.mjs';
import * as syntax from '../scripts/syntax/stage.mjs';
import { createNamer } from '../scripts/model.mjs';
import { openJournal } from '../scripts/journal.mjs';
import { modelService, complete } from './helpers/model-service.mjs';

const exporter = 'function q() { return 42; }\nexport { q as q };\n';
const importer = 'import { q as w } from "./lib.js"; w();\n';
const records = data => data.trim().split('\n').map(line => JSON.parse(line));

// Authenticated requests use the real client and a local Responses transport.
async function fixture(t, files, groups) {
  const root = await mkdtemp(join(tmpdir(), 'unminify-batch-follow-'));
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  for (const [file, source] of Object.entries(files)) {
    await mkdir(join(root, file, '..'), { recursive: true });
    await writeFile(join(root, file), source);
  }
  const chunks = groups ? join(root, 'chunks.json') : undefined;
  if (chunks) await writeFile(chunks, JSON.stringify({ version: 1, chunks: groups }));
  t.after(() => rm(root, { recursive: true, force: true }));
  const controls = { interruptLocal: false }, requests = [], lines = [];
  const service = await modelService(t, root, async (req, res) => {
    let raw = ''; for await (const part of req) raw += part;
    const body = JSON.parse(raw), context = body.input[0].content[0].text;
    requests.push(context);
    if (controls.interruptLocal && /function calculatePending\(v\d+\)/.test(context)) {
      res.writeHead(429, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: { code: 'usage_limit_reached', message: 'local interruption' } }));
      return;
    }
    const names = Object.fromEntries(body.text.format.schema.required.map(key => [key,
      /function calculatePending\(v\d+\)/.test(context) ? 'argumentValue' : 'calculateAnswer']));
    complete(res, names, requests.length);
  });
  return { root, controls, requests, lines,
    read: file => readFile(join(root, file)),
    write: (file, source) => writeFile(join(root, file), source),
    run: (paths, extra = []) => run(parseRunArgs([...paths.map(file => join(root, file)), '--concurrency', '1',
      ...(chunks ? ['--chunks', chunks] : []), ...extra]), {
      ...select, ...rename, ...checks, ...syntax,
      createNamer: options => createNamer({ ...options, ...service, retryDelayMs: 1 }),
      output: line => lines.push(line),
    }),
  };
}

for (const scenario of ['A', 'C', 'C-shared']) test(`${scenario}: separate selections equal one run byte for byte`, async t => {
  const files = { 'lib.js': exporter, 'app.js': importer };
  let groups;
  if (scenario === 'C') {
    files['app.js'] = '// Recovered from consumer.js lines 1-1\nimport { q as w } from "./lib.js";\n';
    files['app-tail.js'] = '// Recovered from consumer.js lines 2-2\nw();\n';
    groups = [{ id: 'consumer', files: ['app.js', 'app-tail.js'] }];
  } else if (scenario === 'C-shared') groups = [{ id: 'consumer', files: ['lib.js', 'app.js'] }];
  const batched = await fixture(t, files, groups), together = await fixture(t, files, groups);
  assert.equal((await batched.run(['app.js'])).exitCode, 0);
  assert.equal((await batched.run(['lib.js'])).exitCode, 0);
  assert.equal((await together.run(Object.keys(files))).exitCode, 0);
  assert.match((await together.read('app.js')).toString(), /q as calculateAnswer/);
  for (const file of Object.keys(files)) assert.deepEqual(await batched.read(file), await together.read(file), file);
  const requests = batched.requests.length;
  assert.equal((await batched.run(['lib.js'])).exitCode, 0);
  assert.equal(batched.requests.length, requests, 'repeating a completed selection does not name again');
});

for (const chunk of [false, true]) for (const condition of ['ignored', 'changed', 'unfinished']) {
  test(`${chunk ? 'chunk' : 'ordinary'} historical ${condition} consumers are never written`, async t => {
    const files = { 'lib.js': exporter, 'app.js': importer, 'ready.js': importer };
    if (chunk) {
      files['app.js'] = '// Recovered from consumer.js lines 1-1\nimport { q as w } from "./lib.js";\n';
      files['app-tail.js'] = '// Recovered from consumer.js lines 2-2\nw();\n';
    }
    const groups = chunk ? [{ id: 'consumer', files: ['app.js', 'app-tail.js'] }] : undefined;
    const f = await fixture(t, files, groups);
    assert.equal((await f.run(['app.js', 'ready.js'])).exitCode, 0);
    const journalPath = chunk
      ? join('.unminify/rename-bindings/chunks', (await readdir(join(f.root, '.unminify/rename-bindings/chunks')))[0])
      : '.unminify/rename-bindings/files/app.js.jsonl';
    if (condition === 'ignored') await f.write('.ignore', 'app.js\n');
    if (condition === 'changed') await f.write('app.js', files['app.js'] + '// user edit\n');
    if (condition === 'unfinished') {
      const history = records((await f.read(journalPath)).toString());
      // A header is a real interrupted journal with no accepted output yet.
      await f.write(journalPath, JSON.stringify(history[0]) + '\n');
    }
    const protectedFiles = chunk ? ['app.js', 'app-tail.js'] : ['app.js'];
    const before = new Map(await Promise.all([...protectedFiles, journalPath].map(async file => [file, await f.read(file)])));
    await f.run(['lib.js']);
    for (const [file, source] of before) assert.deepEqual(await f.read(file), source, `${file} stays untouched`);
    assert.match((await f.read('ready.js')).toString(), /q as calculateAnswer/, 'eligible historical consumer still follows');
    if (condition === 'changed') assert.match(f.lines.join('\n'), /source changed/, 'content mismatch is reported');
    assert.equal(f.requests.length, 1, 'historical files never get model naming');
  });
}

for (const chunk of [false, true]) test(`${chunk ? 'chunk' : 'ordinary'} unfinished exports remain readable without any write`, async t => {
  const pending = 'const q = 42; export { q as pendingValue };\n';
  const files = { 'pending.js': pending, 'fresh.js': 'import { pendingValue as x } from "./pending.js"; console.log(x);\n' };
  if (chunk) files['pending-tail.js'] = '// Pending shared-scope sibling.\n';
  const groups = chunk ? [{ id: 'pending', files: ['pending.js', 'pending-tail.js'] }] : undefined;
  const f = await fixture(t, files, groups);
  let journalPath;
  if (chunk) {
    const { openChunkJournal } = await import('../scripts/chunk-journal.mjs');
    journalPath = openChunkJournal(f.root, 'pending', new Map([['pending.js', pending], ['pending-tail.js', files['pending-tail.js']]])).path;
  } else journalPath = openJournal(f.root, 'pending.js', pending, { minSymbolLength: 4, maxInputTokens: 100000 }).path;
  const before = { source: await f.read('pending.js'), journal: await readFile(journalPath) };
  assert.equal((await f.run(['fresh.js'])).exitCode, 0);
  assert.match((await f.read('fresh.js')).toString(), /pendingValue as q/, 'follows the real current export, not an external-module guess');
  assert.deepEqual(await f.read('pending.js'), before.source);
  assert.deepEqual(await readFile(journalPath), before.journal);
  assert.equal(f.requests.length, 0);
});

test('an unfinished chunk resumes saved program replies after changing selected file', async t => {
  const files = { 'a.js': 'const a = 11;\n', 'b.js': 'function calculatePending(p) { return a + p; } console.log(calculatePending(1));\n' };
  const groups = [{ id: 'pending', files: Object.keys(files) }];
  const f = await fixture(t, files, groups);
  f.controls.interruptLocal = true;
  assert.equal((await f.run(['b.js'])).exitCode, 1);
  const journalPath = join('.unminify/rename-bindings/chunks', (await readdir(join(f.root, '.unminify/rename-bindings/chunks')))[0]);
  const interrupted = records((await f.read(journalPath)).toString());
  assert(interrupted.some(record => record.type === 'chunk-batch' && record.id === 'program-resolved'), 'program reply was saved before interruption');
  assert(!interrupted.some(record => record.type === 'chunk-done'));
  for (const [file, source] of Object.entries(files)) assert.equal((await f.read(file)).toString(), source);
  f.controls.interruptLocal = false;
  assert.equal((await f.run(['a.js'])).exitCode, 0, 'a different selection safely resumes the unfinished chunk');
  assert.equal((await f.run(['b.js'])).exitCode, 0, 'the remaining local naming still runs');
  const together = await fixture(t, files, groups);
  assert.equal((await together.run(Object.keys(files))).exitCode, 0);
  for (const file of Object.keys(files)) assert.deepEqual(await f.read(file), await together.read(file), file);
});

async function runWithoutChunks(t, target, paths) {
  const transport = join(target.root, 'standalone-transport');
  await mkdir(transport);
  const service = await modelService(t, transport, async (req, res) => {
    let raw = ''; for await (const part of req) raw += part;
    const body = JSON.parse(raw);
    complete(res, Object.fromEntries(body.text.format.schema.required.map(key => [key, 'calculateAnswer'])));
  });
  return run(parseRunArgs([...paths.map(file => join(target.root, file)), '--concurrency', '1']), {
    ...select, ...rename, ...checks, ...syntax,
    createNamer: options => createNamer({ ...options, ...service, retryDelayMs: 1 }),
    output: line => target.lines.push(line),
  });
}

test('historical completed chunks follow imports even without a manifest on the later run', async t => {
  const files = { 'lib.js': exporter,
    'app.js': '// Recovered from consumer.js lines 1-1\nimport { q as w } from "./lib.js";\n',
    'app-tail.js': '// Recovered from consumer.js lines 2-2\nw();\n' };
  const groups = [{ id: 'consumer', files: ['app.js', 'app-tail.js'] }];
  const batched = await fixture(t, files, groups), together = await fixture(t, files, groups);
  assert.equal((await batched.run(['app.js'])).exitCode, 0);
  assert.equal((await runWithoutChunks(t, batched, ['lib.js'])).exitCode, 0);
  assert.equal((await together.run(Object.keys(files))).exitCode, 0);
  for (const file of Object.keys(files)) assert.deepEqual(await batched.read(file), await together.read(file), file);
});

for (const condition of ['unfinished', 'ignored']) test(`${condition} import/export intermediaries expose only the names actually on disk`, async t => {
  const bridge = 'import { q as w } from "./lib.js"; export { w as bridgeValue };\n';
  const files = { 'lib.js': exporter, 'bridge.js': bridge,
    'downstream.js': 'import { bridgeValue as x } from "./bridge.js"; console.log(x);\n' };
  const f = await fixture(t, files);
  const journal = openJournal(f.root, 'bridge.js', bridge, { minSymbolLength: 4, maxInputTokens: 100000 });
  if (condition === 'ignored') {
    journal.complete(bridge, 'renamed');
    await f.write('.ignore', 'bridge.js\n');
  }
  const before = { source: await f.read('bridge.js'), journal: await readFile(journal.path) };
  assert.equal((await f.run(['lib.js', 'downstream.js'])).exitCode, 0);
  assert.match((await f.read('lib.js')).toString(), /calculateAnswer as q/);
  assert.deepEqual(await f.read('bridge.js'), before.source);
  assert.deepEqual(await readFile(journal.path), before.journal);
  assert.match((await f.read('downstream.js')).toString(), /bridgeValue as w/, 'never propagates a proposed import name from a read-only intermediary');
});

test('selection changes replay nonempty saved program names without another program request', async t => {
  const files = { 'a.js': 'const a = 11; function calculatePending(p) { return a + p; }\n',
    'b.js': 'console.log(calculatePending(a));\n' };
  const groups = [{ id: 'pending', files: Object.keys(files) }];
  const f = await fixture(t, files, groups);
  f.controls.interruptLocal = true;
  assert.equal((await f.run(['a.js'])).exitCode, 1);
  const journalPath = join('.unminify/rename-bindings/chunks', (await readdir(join(f.root, '.unminify/rename-bindings/chunks')))[0]);
  const interrupted = records((await f.read(journalPath)).toString());
  const savedProgram = interrupted.find(record => record.type === 'chunk-batch' && record.id === 'program-resolved');
  assert(savedProgram && Object.keys(savedProgram.names).length > 0, 'a real program name is saved before local interruption');
  assert(!interrupted.some(record => record.type === 'chunk-done'));
  const programRequests = () => f.requests.filter(context => /const v\d+ = 11/.test(context)).length;
  assert.equal(programRequests(), 1);
  f.controls.interruptLocal = false;
  assert.equal((await f.run(['b.js'])).exitCode, 0);
  assert.equal((await f.run(['a.js'])).exitCode, 0);
  assert.equal(programRequests(), 1, 'saved program names are replayed, not requested again');
  const together = await fixture(t, files, groups);
  assert.equal((await together.run(Object.keys(files))).exitCode, 0);
  for (const file of Object.keys(files)) assert.deepEqual(await f.read(file), await together.read(file), file);
});

test('a model failure in a later chunk selection exits with the selection still pending', async t => {
  const { spawn } = await import('node:child_process');
  const { once } = await import('node:events');
  const root = await mkdtemp(join(tmpdir(), 'unminify-batch-failure-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  const files = { 'a.js': 'const a = 11; function calculatePending(p) { return a + p; }\n',
    'b.js': 'function calculateOther(q) { return q + 1; } console.log(calculateOther(1));\n' };
  for (const [file, source] of Object.entries(files)) await writeFile(join(root, file), source);
  const chunks = join(root, 'chunks.json');
  await writeFile(chunks, JSON.stringify({ version: 1, chunks: [{ id: 'pending', files: Object.keys(files) }] }));
  let phase = 'interrupt', failedRequests = 0;
  const service = await modelService(t, root, async (req, res) => {
    let raw = ''; for await (const part of req) raw += part;
    const body = JSON.parse(raw), context = body.input[0].content[0].text;
    if (phase === 'interrupt' && /function calculatePending\(v\d+\)/.test(context)) {
      res.writeHead(429, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: { code: 'usage_limit_reached', message: 'local interruption' } }));
      return;
    }
    if (phase === 'fail' && /function calculateOther\(v\d+\)/.test(context)) {
      failedRequests++;
      res.writeHead(400, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: { message: 'local request failure' } }));
      return;
    }
    complete(res, Object.fromEntries(body.text.format.schema.required.map(key => [key,
      /function calculatePending\(v\d+\)/.test(context) ? 'argumentValue' : 'calculateAnswer'])));
  });
  const settings = { paths: [join(root, 'a.js')], chunks, concurrency: 1 };
  assert.equal((await run(settings, { ...select, ...rename, ...checks, ...syntax,
    createNamer: options => createNamer({ ...options, ...service, retryDelayMs: 1 }), output: () => {} })).exitCode, 1);
  phase = 'fail';
  const moduleUrl = name => new URL(`../scripts/${name}.mjs`, import.meta.url).href;
  const childSource = `import {run} from ${JSON.stringify(moduleUrl('cli'))};
    import * as select from ${JSON.stringify(moduleUrl('select'))};
    import * as rename from ${JSON.stringify(moduleUrl('rename'))};
    import * as checks from ${JSON.stringify(moduleUrl('checks'))};
    import * as syntax from ${JSON.stringify(moduleUrl('syntax/stage'))};
    import {createNamer} from ${JSON.stringify(moduleUrl('model'))};
    const result=await run(${JSON.stringify({ ...settings, paths: [join(root, 'b.js')] })}, {...select,...rename,...checks,...syntax,
      createNamer: options=>createNamer({...options,...${JSON.stringify(service)},retryDelayMs:1}),output:()=>{}});
    console.log(JSON.stringify(result));`;
  const child = spawn(process.execPath, ['--input-type=module', '-e', childSource], {
    stdio: ['ignore', 'pipe', 'pipe'], timeout: 4000, killSignal: 'SIGKILL',
  });
  t.after(() => { if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL'); });
  let stdout = '', stderr = '';
  child.stdout.on('data', data => { if (stdout.length < 32000) stdout += data; });
  child.stderr.on('data', data => { if (stderr.length < 32000) stderr += data; });
  const [code, signal] = await once(child, 'exit');
  assert.equal(signal, null, 'a failed later selection must exit before the fixed deadline');
  assert.equal(code, 0, stderr);
  assert.equal(JSON.parse(stdout).exitCode, 1);
  assert.equal(failedRequests, 1, 'a failed selection is not scheduled repeatedly');
  assert.equal(await readFile(join(root, 'b.js'), 'utf8'), files['b.js']);
  const directory = join(root, '.unminify/rename-bindings/chunks');
  const history = records(await readFile(join(directory, (await readdir(directory))[0]), 'utf8'));
  assert(history.findLastIndex(record => record.type === 'chunk-selection') > history.findLastIndex(record => record.type === 'chunk-done'),
    'the failed selection remains pending rather than inheriting a prior completion');
});

test('pipeline checkpoints are not historical rename journals', async t => {
  const { journalHistory } = await import('../scripts/history.mjs');
  const f = await fixture(t, { 'lib.js': exporter, 'app.js': importer });
  assert.equal((await f.run(['app.js'])).exitCode, 0);
  assert.deepEqual(journalHistory(f.root).map(entry => entry.file), ['app.js']);
  assert(!f.lines.some(line => line.includes('source changed')), 'pipeline checkpoint paths are never reported as missing source files');
});
