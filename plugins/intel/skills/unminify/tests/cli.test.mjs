import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync, spawnSync } from 'node:child_process';
import { run, parseArgs } from '../scripts/cli.mjs';

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
    renderBatch(source, batch, renames) {
      events.push(['render', batch.id, Object.fromEntries(renames)]);
      return { context: batch.id, placeholders: { v1: batch.keys[0] } };
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
        async name({ placeholders }) {
          calls++; active++; peak = Math.max(peak, active);
          try {
            if (controls.killAt === calls) process.kill(process.pid, 'SIGKILL');
            if (controls.failAt === calls) throw Object.assign(new Error('stopped'), { code: controls.code ?? 'USAGE_LIMIT' });
            await new Promise(resolve => setTimeout(resolve, 5));
            log({ kind: 'model', status: 200, duration_ms: 5, usage: { input_tokens: 10, cached_input_tokens: 2, output_tokens: 3, reasoning_output_tokens: 1 }, rate_headers: { 'x-codex-primary-used-percent': String(calls) } });
            return { names: { v1: names.get(placeholders.v1) }, usage: {} };
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
  for (const args of [[], ['src', '--model', 'other'], ['src', '--max-ids', '0'], ['src', '--short-len', '1.5'], ['src', '--ignore']]) assert.throws(() => parseArgs(args));
});
test('one shared namer, stage ordering, bounded concurrency, completion replay and usage', async t => {
  const root = await workspace(t, { 'a.js': 'a b c', 'b.js': 'a b c' });
  const deps = contracts(['a.js', 'b.js']);
  const result = await run({ dir: root, concurrency: 2 }, deps);
  assert.equal(result.exitCode, 0);
  assert.equal(result.requests, 6);
  assert.deepEqual(result.tokens, { input_tokens: 60, cached_input_tokens: 12, output_tokens: 18, reasoning_output_tokens: 6, total_tokens: 78 });
  assert.equal(result.quota['x-codex-primary-used-percent'].before, 2);
  assert.equal(deps.events.filter(e => e[0] === 'create').length, 1);
  assert.equal(deps.peak, 2);
  for (const event of deps.events.filter(e => e[0] === 'render' && e[1] !== 'top')) assert.equal(event[2]['0'], 'outer');
  assert.equal(await readFile(join(root, 'a.js'), 'utf8'), 'outer first second');
  const replay = contracts(['a.js', 'b.js']);
  await run({ dir: root }, replay);
  assert.equal(replay.calls, 0);
  assert.deepEqual(deps.events.at(-1), ['close', 0]);
});
for (const code of ['AUTH', 'USAGE_LIMIT']) test(`${code} drains in-flight replies, persists them and resumes`, async t => {
  const root = await workspace(t);
  const interrupted = contracts(['a.js'], { failAt: 2, code });
  const result = await run({ dir: root, concurrency: 2 }, interrupted);
  assert.equal(result.exitCode, 1);
  assert.equal(await readFile(join(root, 'a.js'), 'utf8'), 'a b c');
  assert.match(interrupted.lines.join('\n'), /Resume/);
  assert.deepEqual(interrupted.events.at(-1), ['close', 0]);
  const resumed = contracts();
  await run({ dir: root }, resumed);
  assert.equal(resumed.calls, 1);
  assert.equal(await readFile(join(root, 'a.js'), 'utf8'), 'outer first second');
});
test('SIGKILL after N replies resumes to byte-identical output', async t => {
  const root = await workspace(t);
  const child = `import {run} from ${JSON.stringify(new URL('../scripts/cli.mjs', import.meta.url).href)}; import {contracts} from ${JSON.stringify(import.meta.url)}; await run({dir:${JSON.stringify(root)},concurrency:1},contracts(['a.js'],{killAt:3}));`;
  // Keep the child isolated from node:test registrations.
  const fixture = join(root, 'contracts.mjs');
  await writeFile(fixture, `export ${contracts.toString()}`);
  const script = child.replace(JSON.stringify(import.meta.url), JSON.stringify(new URL(`file://${fixture}`).href));
  const killed = spawnSync(process.execPath, ['--input-type=module', '-e', script]);
  assert.equal(killed.signal, 'SIGKILL', killed.stderr?.toString());
  assert.equal(await readFile(join(root, 'a.js'), 'utf8'), 'a b c');
  const resumed = contracts();
  await run({ dir: root, concurrency: 1 }, resumed);
  assert.equal(resumed.calls, 1);
  const uninterrupted = await workspace(t);
  await run({ dir: uninterrupted, concurrency: 1 }, contracts());
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
  const result = await run({ dir: root }, deps);
  assert.equal(result.exitCode, 1);
  assert.equal(await readFile(join(root, 'a.js'), 'utf8'), 'outer first second');
  assert.equal(await readFile(join(root, 'b.js'), 'utf8'), 'safe import');
  assert.match(deps.lines.join('\n'), /captured reference/);
  const rejectedRoot = await workspace(t);
  await run({ dir: rejectedRoot }, contracts(['a.js'], { checkRewrite: () => 'bad binding' }));
  assert.equal(await readFile(join(rejectedRoot, 'a.js'), 'utf8'), 'a b c');
});

test('completed rejected files remain visible as failures on resume', async t => {
  const root = await workspace(t);
  await run({ dir: root }, contracts(['a.js'], { checkRewrite: () => 'bad binding' }));
  const resumed = contracts();
  const result = await run({ dir: root }, resumed);
  assert.equal(resumed.calls, 0);
  assert.equal(result.rejected, 1);
  assert.equal(result.exitCode, 1);
  assert.match(resumed.lines.join('\n'), /bad binding/);
});
