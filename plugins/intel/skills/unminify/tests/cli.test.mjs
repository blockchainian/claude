import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, writeFile, rm, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { run, parseArgs } from '../scripts/cli.mjs';

test('CLI runs through a symlink to the scripts directory', async t => {
  const root = await mkdtemp(join(tmpdir(), 'unminify-entry-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const scripts = join(root, 'scripts');
  await symlink(fileURLToPath(new URL('../scripts', import.meta.url)), scripts, 'dir');
  const result = spawnSync(process.execPath, [join(scripts, 'cli.mjs'), '--help'], { encoding: 'utf8' });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /unminify <path>/);
});

async function workspace(t, files = { 'a.js': 'a b c' }) {
  const root = await mkdtemp(join(tmpdir(), 'unminify-cli-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  for (const [file, source] of Object.entries(files)) await writeFile(join(root, file), source);
  return root;
}
// Contract implementations isolate scheduling and persistence from parsing and model naming.
export function contracts(files = ['a.js'], controls = {}) {
  const names = new Map([[0, 'outer'], [2, 'first'], [4, 'second']]);
  const events = [], output = [];
  let calls = 0, active = 0, peak = 0;
  return {
    events, get calls() { return calls; }, get peak() { return peak; },
    listFiles: async () => files,
    planFile: () => ({ stages: [[{ id: 'top', keys: [0] }], [{ id: 'first', keys: [2] }, { id: 'second', keys: [4] }]] }),
    renderBatch(source, batch, renames, { placeholderOffset = 0 } = {}) {
      events.push(['render', batch.id, Object.fromEntries(renames)]);
      return { context: batch.id, placeholders: Object.fromEntries(batch.keys.map((key, index) => [`v${placeholderOffset + index + 1}`, key])) };
    },
    resolveNames(source, proposals) { events.push(['resolve']); return new Map(proposals); },
    applyRenames(source, renames) {
      return source.split('').map((letter, key) => renames.get(key) ?? letter).join('');
    },
    followImports: controls.followImports ?? (() => new Map()),
    checkRewrite: controls.checkRewrite ?? (() => null),
    createNamer({ log }) {
      events.push(['create']);
      return {
        async name({ placeholders }, telemetry) {
          const call = ++calls;
          events.push(['request', telemetry.file, telemetry.batch_ids, { ...placeholders }]);
          active++; peak = Math.max(peak, active);
          try {
            if (controls.killAt === calls) process.kill(process.pid, 'SIGKILL');
            if (controls.failAt === calls) throw Object.assign(new Error('stopped'), { code: controls.code ?? 'USAGE_LIMIT' });
            await new Promise(resolve => setTimeout(resolve, 5));
            events.push(['reply', telemetry.file, telemetry.batch_ids]);
            log({ ...telemetry, kind: 'model', status: 200, duration_ms: 5, usage: { input_tokens: 10, cached_input_tokens: 2, output_tokens: 3, reasoning_output_tokens: 1 }, rate_headers: { 'x-codex-primary-used-percent': String(call) } });
            return { names: Object.fromEntries(Object.entries(placeholders).map(([placeholder, key]) => [placeholder, names.get(key)])), usage: {} };
          } finally { active--; }
        },
        close() { events.push(['close', active]); },
      };
    },
    output: line => output.push(line), lines: output,
  };
}
test('arguments enforce the fixed model and positive integer limits', () => {
  assert.equal(parseArgs(['src', '--concurrency', '2']).concurrency, 2);
  assert.equal(parseArgs(['src']).contextTokens, 100000);
  assert.deepEqual(parseArgs(['src/a', '--concurrency', '2', 'src/b']).paths.map(dir => dir.split('/').slice(-2).join('/')), ['src/a', 'src/b']);
  for (const args of [[], ['src', '--model', 'other'], ['src', '--max-ids', '60'], ['src', '--context-bytes', '8000'], ['src', '--context-tokens', '0'], ['src', '--short-len', '1.5'], ['src', '--ignore']]) assert.throws(() => parseArgs(args));
});
test('one run renames the files of every given path once', async t => {
  const root = await workspace(t, { 'a.js': 'a b c', 'b.js': 'a b c' });
  const deps = contracts();
  const listed = [];
  for (const dir of ['one', 'two']) await mkdir(join(root, dir));
  deps.listFiles = async dir => { listed.push(dir); return dir.endsWith('one') ? ['a.js'] : ['a.js', 'b.js']; };
  const result = await run({ paths: [join(root, 'one'), join(root, 'two')] }, deps);
  assert.deepEqual(listed, [join(root, 'one'), join(root, 'two')]);
  assert.equal(result.files, 2);
  assert.equal(result.completed, 2);
});
test('one shared namer, stage ordering, bounded concurrency, completion replay and usage', async t => {
  const root = await workspace(t, { 'a.js': 'a b c', 'b.js': 'a b c' });
  const deps = contracts(['a.js', 'b.js']);
  const result = await run({ paths: [root], concurrency: 2 }, deps);
  assert.equal(result.exitCode, 0);
  assert.equal(result.requests, 4);
  assert.deepEqual(result.tokens, { input_tokens: 40, cached_input_tokens: 8, output_tokens: 12, reasoning_output_tokens: 4, total_tokens: 52 });
  assert.equal(result.quota['x-codex-primary-used-percent'].before, 1);
  assert.equal(deps.events.filter(e => e[0] === 'create').length, 1);
  assert.equal(deps.peak, 2);
  for (const event of deps.events.filter(e => e[0] === 'render' && e[1] !== 'top')) assert.equal(event[2]['0'], 'outer');
  for (const file of ['a.js', 'b.js']) {
    assert.equal(await readFile(join(root, file), 'utf8'), 'outer first second');
    assert.deepEqual(deps.events.filter(e => e[0] === 'request' && e[1] === file).map(e => [e[2], e[3]]), [
      [['top'], { v1: 0 }], [['first', 'second'], { v1: 2, v2: 4 }],
    ]);
    const topReply = deps.events.findIndex(e => e[0] === 'reply' && e[1] === file && e[2][0] === 'top');
    const functionRequest = deps.events.findIndex(e => e[0] === 'request' && e[1] === file && e[2][0] === 'first');
    assert(topReply < functionRequest);
  }
  const replay = contracts(['a.js', 'b.js']);
  await run({ paths: [root] }, replay);
  assert.equal(replay.calls, 0);
  assert.deepEqual(deps.events.at(-1), ['close', 0]);
});
test('a failed batch leaves its file unwritten and unfinished so resume retries it', async t => {
  const root = await workspace(t);
  const failed = contracts(['a.js'], { failAt: 2, code: 'NETWORK' });
  const result = await run({ paths: [root], concurrency: 1 }, failed);
  assert.equal(result.exitCode, 1);
  assert.equal(result.failedBatches, 2);
  assert.equal(await readFile(join(root, 'a.js'), 'utf8'), 'a b c');
  const resumed = contracts(['a.js']);
  assert.equal((await run({ paths: [root] }, resumed)).exitCode, 0);
  assert.equal(resumed.calls, 1);
  assert.equal(await readFile(join(root, 'a.js'), 'utf8'), 'outer first second');
});
for (const code of ['AUTH', 'USAGE_LIMIT']) test(`${code} drains in-flight replies, persists them and resumes`, async t => {
  const files = ['a.js', 'b.js'];
  const root = await workspace(t, { 'a.js': 'a b c', 'b.js': 'a b c' });
  const interrupted = contracts(files, { failAt: 4, code });
  const result = await run({ paths: [root], concurrency: 2 }, interrupted);
  assert.equal(result.exitCode, 1);
  assert.equal(interrupted.calls, 4);
  assert.equal(interrupted.peak, 2);
  assert.equal(await readFile(join(root, 'a.js'), 'utf8'), 'outer first second');
  assert.equal(await readFile(join(root, 'b.js'), 'utf8'), 'a b c');
  assert.match(interrupted.lines.join('\n'), /Resume/);
  assert.deepEqual(interrupted.events.at(-1), ['close', 0]);
  const failureRequest = interrupted.events.findIndex(e => e[0] === 'request' && e[1] === 'b.js' && e[2][0] === 'first');
  const drainedReply = interrupted.events.findIndex(e => e[0] === 'reply' && e[1] === 'a.js' && e[2][0] === 'first');
  assert(drainedReply > failureRequest, 'the other packed request completes after the terminal failure');
  const journal = async file => (await readFile(join(root, '.unminify', `${file}.jsonl`), 'utf8'))
    .trim().split('\n').map(line => JSON.parse(line)).filter(record => record.id);
  assert.deepEqual((await journal('a.js')).map(record => record.id), ['top', 'first', 'second']);
  assert.deepEqual((await journal('b.js')).map(record => record.id), ['top']);
  const resumed = contracts(files);
  const resumedResult = await run({ paths: [root] }, resumed);
  assert.equal(resumedResult.exitCode, 0);
  assert.equal(resumed.calls, 1);
  assert.deepEqual(resumed.events.filter(e => e[0] === 'request'), [
    ['request', 'b.js', ['first', 'second'], { v1: 2, v2: 4 }],
  ]);
  for (const file of files) {
    assert.equal(await readFile(join(root, file), 'utf8'), 'outer first second');
    assert.deepEqual((await journal(file)).map(record => record.id), ['top', 'first', 'second']);
  }
});
test('SIGKILL after N replies resumes to byte-identical output', async t => {
  const root = await workspace(t);
  const child = `import {run} from ${JSON.stringify(new URL('../scripts/cli.mjs', import.meta.url).href)}; import {contracts} from ${JSON.stringify(import.meta.url)}; await run({paths:[${JSON.stringify(root)}],concurrency:1},contracts(['a.js'],{killAt:2}));`;
  // Keep the child isolated from node:test registrations.
  const fixture = join(root, 'contracts.mjs');
  await writeFile(fixture, `export ${contracts.toString()}`);
  const script = child.replace(JSON.stringify(import.meta.url), JSON.stringify(new URL(`file://${fixture}`).href));
  const killed = spawnSync(process.execPath, ['--input-type=module', '-e', script]);
  assert.equal(killed.signal, 'SIGKILL', killed.stderr?.toString());
  assert.equal(await readFile(join(root, 'a.js'), 'utf8'), 'a b c');
  const resumed = contracts();
  await run({ paths: [root], concurrency: 1 }, resumed);
  assert.equal(resumed.calls, 1);
  assert.deepEqual(resumed.events.filter(e => e[0] === 'request'), [
    ['request', 'a.js', ['first', 'second'], { v1: 2, v2: 4 }],
  ]);
  assert.equal(await readFile(join(root, 'a.js'), 'utf8'), 'outer first second');
  const uninterrupted = await workspace(t);
  await run({ paths: [uninterrupted], concurrency: 1 }, contracts());
  assert.deepEqual(await readFile(join(root, 'a.js')), await readFile(join(uninterrupted, 'a.js')));
});
test('rewrite failures preserve originals and import following is independently gated', async t => {
  const root = await workspace(t, { 'a.js': 'a b c', 'b.js': 'a b c' });
  const deps = contracts(['a.js', 'b.js'], {
    followImports(files) {
      assert.equal(files.get('a.js'), 'outer first second');
      return new Map([['a.js', 'unsafe import'], ['b.js', 'safe import']]);
    },
    checkRewrite(before, after) { return after === 'unsafe import' ? 'captured reference' : null; },
  });
  const result = await run({ paths: [root] }, deps);
  assert.equal(result.exitCode, 1);
  assert.equal(await readFile(join(root, 'a.js'), 'utf8'), 'outer first second');
  assert.equal(await readFile(join(root, 'b.js'), 'utf8'), 'safe import');
  assert.match(deps.lines.join('\n'), /captured reference/);
  const rejectedRoot = await workspace(t);
  await run({ paths: [rejectedRoot] }, contracts(['a.js'], { checkRewrite: () => 'bad binding' }));
  assert.equal(await readFile(join(rejectedRoot, 'a.js'), 'utf8'), 'a b c');
});

test('completed rejected files remain visible as failures on resume', async t => {
  const root = await workspace(t);
  await run({ paths: [root] }, contracts(['a.js'], { checkRewrite: () => 'bad binding' }));
  const resumed = contracts();
  const result = await run({ paths: [root] }, resumed);
  assert.equal(resumed.calls, 0);
  assert.equal(result.rejected, 1);
  assert.equal(result.exitCode, 1);
  assert.match(resumed.lines.join('\n'), /bad binding/);
});
