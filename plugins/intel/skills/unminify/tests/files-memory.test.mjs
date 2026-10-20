import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http2';
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { run } from '../scripts/cli.mjs';
import * as select from '../scripts/select.mjs';
import * as rename from '../scripts/rename.mjs';
import * as checks from '../scripts/checks.mjs';
import { createNamer } from '../scripts/model.mjs';
import { withTimings } from '../scripts/timing.mjs';

// Exercise the complete pipeline against a local Responses HTTP/2 service.
async function fixture(t, count) {
  const root = await mkdtemp(join(tmpdir(), 'unminify-files-'));
  const codexHome = join(root, '.auth');
  await mkdir(codexHome);
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  const token = `e30.${Buffer.from(JSON.stringify({ exp: Date.now() / 1000 + 3600 })).toString('base64url')}.local`;
  await writeFile(join(codexHome, 'auth.json'), JSON.stringify({ auth_mode: 'chatgpt',
    tokens: { access_token: token, account_id: 'local-memory-test' } }));
  const source = 'export const a = 1; function calculate(b) { return a + b; }\n'
    + Array.from({ length: 100 }, (_, i) => `function unchanged${i}(inputValue) { return inputValue + ${i}; }`).join('\n');
  for (let i = 0; i < count; i++) await writeFile(join(root, `${i}.mjs`), source);
  let active = 0, peak = 0, requests = 0;
  const server = createServer(async (req, res) => {
    let raw = ''; for await (const chunk of req) raw += chunk;
    const body = JSON.parse(raw);
    active++; requests++; peak = Math.max(peak, active);
    await new Promise(resolve => setTimeout(resolve, 10));
    const names = Object.fromEntries(body.text.format.schema.required.map((key, i) => [key, `readableValue${i}`]));
    res.end(`data: ${JSON.stringify({ type: 'response.completed', response: { status: 'completed',
      output: [{ type: 'message', role: 'assistant', content: [{ type: 'output_text', text: JSON.stringify(names) }] }],
      usage: { input_tokens: 12, output_tokens: 3 } } })}\n\n`);
    active--;
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(async () => { await new Promise(resolve => server.close(resolve)); await rm(root, { recursive: true, force: true }); });
  const plans = [];
  const deps = { ...select, ...rename, ...checks, output: () => {},
    planFile(source, options) { const plan = select.planFile(source, options); plans.push({ source, ...plan }); return plan; },
    createNamer: options => createNamer({ ...options, codexHome,
      baseUrl: `http://127.0.0.1:${server.address().port}`, refreshUrl: `http://127.0.0.1:${server.address().port}/oauth/token` }) };
  return { root, plans, deps, get peak() { return peak; }, get requests() { return requests; } };
}

test('hundreds of files bound planning through writeback while filling request slots and resuming', { timeout: 120_000 }, async t => {
  const target = await fixture(t, 200);
  const summary = await run({ dirs: [target.root], concurrency: 2 }, target.deps);
  assert.equal(summary.exitCode, 0);
  assert.equal(summary.completed, 200);
  assert.equal(target.peak, 2);
  const events = (await readFile(join(target.root, '.unminify/performance.jsonl'), 'utf8')).trim().split('\n').map(JSON.parse);
  const active = new Set(); let peak = 0;
  for (const event of events) {
    if (event.type === 'phase_start' && event.phase === 'selection_plan' && event.file) active.add(event.file);
    if (event.type === 'file_end' && event.phase === 'main') active.delete(event.file);
    peak = Math.max(peak, active.size);
  }
  assert.equal(active.size, 0);
  assert(peak <= 4, `at most twice request concurrency may retain plans; saw ${peak}`);
  const requests = target.requests;
  const replay = await run({ dirs: [target.root], concurrency: 2 }, target.deps);
  assert.equal(replay.skipped, 200);
  assert.equal(target.requests, requests);
});

test('finished files detach cached ASTs even when callers retain their batch objects', async t => {
  const target = await fixture(t, 2);
  assert.equal((await run({ dirs: [target.root], concurrency: 1 }, target.deps)).exitCode, 0);
  for (const { source, stages } of target.plans) {
    const totals = {};
    withTimings(totals, () => select.renderBatch(source, stages.flat()[0]));
    assert(totals.parse > 0, 'a retained batch must no longer retain its finished file analysis');
  }
});
