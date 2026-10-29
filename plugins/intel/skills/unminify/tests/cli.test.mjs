import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, writeFile, rm, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync, spawnSync, spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { run, parseArgs } from '../scripts/cli.mjs';
import * as select from '../scripts/select.mjs';
import * as rename from '../scripts/rename.mjs';
import * as checks from '../scripts/checks.mjs';
import * as syntax from '../scripts/syntax/stage.mjs';
import { createNamer } from '../scripts/model.mjs';
import { modelService, complete } from './helpers/model-service.mjs';

// Original, executable programs: one top-level and two independently packed function batches.
const source = 'const a = 10;\nfunction first(p) { return a + p; }\nfunction second(q) { return a * q; }\nconsole.log(first(2), second(3));\n'
  + `function fixtureDescription() { return "${'This fixture calculates two totals. '.repeat(80)}"; }\n`;
const expected = source.replaceAll(/\ba\b/g, 'outerValue').replaceAll(/\bp\b/g, 'firstValue').replaceAll(/\bq\b/g, 'secondValue');
const records = data => data.trim().split('\n').filter(Boolean).map(line => JSON.parse(line));
async function workspace(t, files = { 'a.js': source }) {
  const root = await mkdtemp(join(tmpdir(), 'unminify-cli-'));
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  for (const [file, text] of Object.entries(files)) {
    await mkdir(join(root, file, '..'), { recursive: true });
    await writeFile(join(root, file), text);
  }
  t.after(() => rm(root, { recursive: true, force: true }));
  return root;
}
async function fixture(t, files, controls = {}) {
  const root = await workspace(t, files);
  const events = [], lines = [];
  let active = 0, peak = 0, sessions = new Set(), refreshes = 0;
  const requests = [];
  const service = await modelService(t, root, async (req, res) => {
    if (req.url === '/oauth/token') {
      refreshes++;
      res.writeHead(401); res.end('{}'); return;
    }
    let raw = ''; for await (const chunk of req) raw += chunk;
    const body = JSON.parse(raw);
    const context = body.input[0].content[0].text;
    const request = { body, context, stage: /const v\d+ = 10/.test(context) ? 'top' : 'functions' };
    requests.push(request);
    events.push(['request', request]);
    sessions.add(req.stream.session);
    active++; peak = Math.max(peak, active);
    try {
      if (await controls.handle?.({ req, res, request, requests, events, root })) return;
      // Hold accepted calls long enough for the other HTTP/2 stream to arrive.
      await new Promise(resolve => setTimeout(resolve, 25));
      const names = Object.fromEntries(body.text.format.schema.required.map(key => [key,
        new RegExp(`const ${key} = 10`).test(context) ? 'outerValue'
          : new RegExp(`first\\(${key}\\)`).test(context) ? 'firstValue' : 'secondValue']));
      complete(res, names, requests.length);
      events.push(['reply', request]);
    } finally { active--; }
  });
  const options = { ...service, refreshUrl: `${service.baseUrl}/oauth/token`, retryDelayMs: 1 };
  return { root, requests, events, lines, options, controls,
    get peak() { return peak; }, get active() { return active; }, get sessions() { return sessions.size; }, get refreshes() { return refreshes; },
    read: file => readFile(join(root, file), 'utf8'),
    journal: async file => records(await readFile(join(root, '.unminify', 'rename-bindings', 'files', `${file}.jsonl`), 'utf8')),
    run: settings => run({ paths: [root], concurrency: 2, maxInputTokens: 200, ...settings }, {
      ...select, ...rename, ...checks, ...syntax, output: line => lines.push(line),
      createNamer: settings => createNamer({ ...settings, ...options }),
    }),
  };
}
async function assertOutput(target, file = 'a.js', text = expected) {
  assert.equal(await target.read(file), text);
  assert.equal(checks.checkRewrite(source, text), null);
  assert.equal((await target.journal(file)).at(-1).status, 'renamed');
}
const batches = journal => journal.filter(record => record.id);

test('CLI runs through a symlink to the scripts directory', async t => {
  const root = await workspace(t, {});
  const scripts = join(root, 'scripts');
  await symlink(fileURLToPath(new URL('../scripts', import.meta.url)), scripts, 'dir');
  const result = spawnSync(process.execPath, [join(scripts, 'cli.mjs'), '--help'], { encoding: 'utf8' });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /unminify restore-syntax <path>/);
});
test('arguments enforce the fixed model and positive integer limits', () => {
  assert.equal(parseArgs(['rename-bindings', 'src', '--concurrency', '2']).concurrency, 2);
  assert.equal(parseArgs(['rename-bindings', 'src']).maxInputTokens, 100000);
  assert.deepEqual(parseArgs(['rename-bindings', 'src/a', '--concurrency', '2', 'src/b']).paths.map(dir => dir.split('/').slice(-2).join('/')), ['src/a', 'src/b']);
  for (const args of [[], ['rename-bindings', 'src', '--model', 'other'], ['rename-bindings', 'src', '--max-ids', '60'], ['rename-bindings', 'src', '--context-bytes', '8000'], ['rename-bindings', 'src', '--max-input-tokens', '0'], ['rename-bindings', 'src', '--min-symbol-length', '1.5'], ['rename-bindings', 'src', '--ignore']]) assert.throws(() => parseArgs(args));
});
test('one run renames the files of every given path once', async t => {
  const target = await fixture(t, { 'one/a.js': source, 'two/b.js': source });
  const result = await target.run({ paths: [join(target.root, 'one'), join(target.root, 'one/a.js'), join(target.root, 'two')] });
  assert.equal(result.files, 2);
  assert.equal(result.completed, 2);
  assert.equal(target.requests.length, 4);
  for (const file of ['one/a.js', 'two/b.js']) await assertOutput(target, file);
});
test('one shared namer, stage ordering, bounded concurrency, completion replay and usage', async t => {
  const target = await fixture(t, { 'a.js': source, 'b.js': source });
  const result = await target.run();
  assert.equal(result.exitCode, 0);
  assert.equal(result.requests, 4);
  assert.deepEqual(result.tokens, { input_tokens: 40, cached_input_tokens: 8, output_tokens: 12, reasoning_output_tokens: 4, total_tokens: 52 });
  assert.equal(result.quota['x-codex-primary-used-percent'].before, 2);
  assert.equal(target.sessions, 1);
  assert.equal(target.peak, 2);
  assert.equal(target.active, 0);
  for (const file of ['a.js', 'b.js']) {
    await assertOutput(target, file);
    const saved = batches(await target.journal(file));
    assert.equal(saved.length, 3);
    assert.deepEqual(saved.map(record => Object.values(record.names)), [['outerValue'], ['firstValue'], ['secondValue']]);
    const syntaxJournal = records(await target.read(`.unminify/restore-syntax/syntax/${file}.syntax.jsonl`));
    assert.equal(syntaxJournal.at(-1).status, 'syntax');
  }
  for (const request of target.requests.filter(request => request.stage === 'functions')) {
    assert.match(request.context, /outerValue/);
    assert.deepEqual(request.body.text.format.schema.required, ['v1', 'v2']);
    assert(target.events.findIndex(e => e[0] === 'reply' && e[1].stage === 'top') < target.events.findIndex(e => e[0] === 'request' && e[1] === request));
  }
  const telemetry = records(await target.read('.unminify/rename-bindings/requests.jsonl'));
  for (const file of ['a.js', 'b.js']) {
    const calls = telemetry.filter(r => r.file === file);
    assert.deepEqual(calls.map(r => r.batch_ids.length), [1, 2]);
    assert(calls[0].ended_at <= calls[1].sent_at, 'each file saves its top-level reply before requesting functions');
    assert.deepEqual(calls.flatMap(r => r.batch_ids), batches(await target.journal(file)).map(r => r.id));
  }
  assert.equal(new Set(telemetry.map(r => r.session_id)).size, 1);
  assert.equal(records(await target.read('.unminify/rename-bindings/performance.jsonl')).at(-1).requests, 4);
  assert(target.lines.map(line => { try { return JSON.parse(line); } catch { return {}; } }).some(r => r.type === 'summary' && r.requests === 4));
  assert.equal((await target.run()).requests, 0);
  assert.equal(target.requests.length, 4);
});
test('a failed NETWORK batch leaves its file unwritten and unfinished so resume retries it', async t => {
  const controls = { fail: true, handle: ({ req, request }) => {
    if (controls.fail && request.stage === 'functions') { req.stream.close(2); return true; }
  } };
  const target = await fixture(t, undefined, controls);
  const result = await target.run({ concurrency: 1 });
  assert.equal(result.exitCode, 1);
  assert.equal(result.failedBatches, 2);
  assert.equal(await target.read('a.js'), source);
  assert.equal(batches(await target.journal('a.js')).length, 1);
  assert(!(await target.journal('a.js')).some(r => r.type === 'done'));
  assert.equal(target.requests.length, 5, 'four real client retries after the saved top-level request');
  controls.fail = false;
  assert.equal((await target.run()).exitCode, 0);
  assert.equal(target.requests.length, 6);
  await assertOutput(target);
});
for (const code of ['AUTH', 'USAGE_LIMIT']) test(`${code} drains in-flight replies, persists them and resumes`, async t => {
  let release, failed = false;
  const failure = new Promise(resolve => { release = resolve; });
  const controls = { handle: async ({ res, request, requests, events }) => {
    if (failed || request.stage !== 'functions') return false;
    if (requests.filter(r => r.stage === 'functions').length === 1) { await failure; return false; }
    failed = true;
    events.push(['failure', request]);
    res.writeHead(code === 'AUTH' ? 401 : 429, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ error: { code: 'usage_limit_reached' } }));
    release(); return true;
  } };
  const target = await fixture(t, { 'a.js': source, 'b.js': source }, controls);
  assert.equal((await target.run()).exitCode, 1);
  assert.equal(target.requests.length, 4);
  assert.equal(target.peak, 2);
  assert.equal(target.active, 0);
  assert.equal(target.refreshes, code === 'AUTH' ? 1 : 0);
  const logs = records(await target.read('.unminify/rename-bindings/requests.jsonl'));
  const failedFile = logs.find(r => r.status === (code === 'AUTH' ? 401 : 429)).file;
  const doneFile = failedFile === 'a.js' ? 'b.js' : 'a.js';
  await assertOutput(target, doneFile);
  assert.equal(await target.read(failedFile), source);
  assert.equal(batches(await target.journal(doneFile)).length, 3);
  assert.equal(batches(await target.journal(failedFile)).length, 1);
  assert.match(target.lines.join('\n'), new RegExp(`${code}: .*Resume`));
  assert(target.events.findIndex(e => e[0] === 'reply' && e[1].stage === 'functions') > target.events.findIndex(e => e[0] === 'failure'));
  assert.equal((await target.run()).exitCode, 0);
  assert.equal(target.requests.length, 5);
  for (const file of ['a.js', 'b.js']) { await assertOutput(target, file); assert.equal(batches(await target.journal(file)).length, 3); }
});
test('SIGKILL after N replies resumes to byte-identical output', async t => {
  let child;
  const target = await fixture(t, undefined, { handle: ({ request }) => {
    if (request.stage === 'functions') { child.kill('SIGKILL'); return true; }
  } });
  const script = `import {run} from ${JSON.stringify(new URL('../scripts/cli.mjs', import.meta.url).href)};
    import * as select from ${JSON.stringify(new URL('../scripts/select.mjs', import.meta.url).href)};
    import * as rename from ${JSON.stringify(new URL('../scripts/rename.mjs', import.meta.url).href)};
    import * as checks from ${JSON.stringify(new URL('../scripts/checks.mjs', import.meta.url).href)};
    import * as syntax from ${JSON.stringify(new URL('../scripts/syntax/stage.mjs', import.meta.url).href)};
    import {createNamer} from ${JSON.stringify(new URL('../scripts/model.mjs', import.meta.url).href)};
    await run({paths:[${JSON.stringify(target.root)}], concurrency:1, maxInputTokens:200}, {...select,...rename,...checks,...syntax,
      createNamer: options => createNamer({...options,...${JSON.stringify(target.options)}})});`;
  child = spawn(process.execPath, ['--input-type=module', '-e', script], { stdio: ['ignore', 'ignore', 'pipe'] });
  t.after(() => { if (child.exitCode === null) child.kill('SIGKILL'); });
  let stderr = ''; child.stderr.on('data', part => { stderr += part; });
  const [, signal] = await once(child, 'exit');
  assert.equal(signal, 'SIGKILL', stderr);
  assert.equal(await target.read('a.js'), source);
  assert.equal(batches(await target.journal('a.js')).length, 1);
  target.controls.handle = undefined;
  assert.equal((await target.run()).exitCode, 0);
  assert.equal(target.requests.length, 3);
  const uninterrupted = await fixture(t);
  await uninterrupted.run();
  assert.equal(await target.read('a.js'), await uninterrupted.read('a.js'));
  await assertOutput(target);
});
// A sloppy-script duplicate parameter is legal JS, but Node's module check must reject it.
const rejected = 'function calculate(a, a) { return a + 1; }\nconsole.log(calculate(1, 2));\n';
test('rewrite failures preserve originals and import following is independently gated', async t => {
  const target = await fixture(t, {
    'a.js': 'export const a = 10;\n',
    'b.js': "import { a as x } from './a.js'; console.log(x);\n",
    'rejected.js': rejected,
  });
  const result = await target.run();
  assert.equal(result.exitCode, 1);
  assert.equal(result.rejected, 1);
  assert.equal(await target.read('rejected.js'), rejected);
  assert.equal((await target.journal('rejected.js')).at(-1).status, 'rejected');
  assert.match(await target.read('a.js'), /export \{ outerValue as a \}/);
  assert.match(await target.read('b.js'), /a as outerValue/);
  assert.equal((await target.journal('b.js')).at(-1).status, 'imports');
  assert.match(target.lines.join('\n'), /PARSE_FAIL/);
  // resolveNames prevents collisions before CLI checks; exercise the real gate with unsafe edits.
  const before = 'const a = 1; const b = 2; console.log(a, b);\n';
  const after = rename.applyRenames(before, new Map([[6, 'b']]));
  assert.equal(checks.checkRewrite(before, after), 'PARSE_FAIL');
  const scopeBefore = 'const a = 1; function calculate(b) { return a + b; } console.log(calculate(2));\n';
  const scopeAfter = rename.applyRenames(scopeBefore, new Map([[scopeBefore.indexOf('(b') + 1, 'a']]));
  assert.equal(checks.checkRewrite(scopeBefore, scopeAfter), 'BINDING_CHANGED');
  const importsBefore = "import { value as x } from './a.js'; const other = 1; console.log(x, other);\n";
  const importsAfter = rename.applyRenames(importsBefore, new Map([[importsBefore.indexOf('x'), 'other']]));
  assert.equal(checks.checkRewrite(importsBefore, importsAfter), 'PARSE_FAIL');
});
test('completed rejected files remain visible as failures on resume', async t => {
  const target = await fixture(t, { 'a.js': rejected });
  assert.equal((await target.run()).rejected, 1);
  const count = target.requests.length;
  const journal = await target.read('.unminify/rename-bindings/files/a.js.jsonl');
  const result = await target.run();
  assert.equal(target.requests.length, count);
  assert.equal(result.rejected, 1);
  assert.equal(result.exitCode, 1);
  assert.match(target.lines.join('\n'), /PARSE_FAIL/);
  assert.equal(await target.read('a.js'), rejected);
  assert.equal(await target.read('.unminify/rename-bindings/files/a.js.jsonl'), journal);
});
