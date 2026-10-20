import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http2';
import { once } from 'node:events';
import { mkdtemp, readFile, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { parse } from '../scripts/node_modules/@babel/parser/lib/index.js';
import { packBatches } from '../scripts/pack.mjs';
import { withTimings } from '../scripts/timing.mjs';
import * as select from '../scripts/select.mjs';
import * as rename from '../scripts/rename.mjs';
import * as checks from '../scripts/checks.mjs';
import { run } from '../scripts/cli.mjs';
import { createNamer } from '../scripts/model.mjs';

const largeSource = count => Array.from({ length: count }, (_, index) =>
  `function operation${index}(a) { const b = a + ${index}; return { a, b }; }`).join('\n');

test('planned large-file batches reuse parsing and keep renders independent', () => {
  const source = Array.from({ length: 100 }, (_, index) => `const a${index} = ${index};`).join('\n')
    + '\n' + largeSource(1600);
  assert(Buffer.byteLength(source) > 100_000);
  const totals = {};
  const plan = withTimings(totals, () => select.planFile(source, { contextTokens: 2000 }));
  assert(totals.parse > 0);
  const batches = plan.stages[1];
  assert.equal(batches.length, 1600);
  const batch = batches.at(-1);
  const renames = new Map([...plan.stages[0].flatMap(batch => batch.keys).map((key, index) => [key, `outer${index}`]),
    ...batches.slice(0, 500).flatMap((batch, index) =>
      batch.keys.map((key, position) => [key, `resolved${index}_${position}`]))]);
  const rendering = {};
  const started = performance.now();
  const first = withTimings(rendering, () => select.renderBatch(source, batch, renames));
  const second = withTimings(rendering, () => select.renderBatch(source, batch));
  assert(performance.now() - started < 500, 'unrelated global names must not cause repeated whole-file scope traversals');
  assert.deepEqual(second, first, 'unrelated resolved names do not change this context');
  assert.equal(rendering.parse ?? 0, 0, 'rendering planned batches must not reparse the full source');
  assert.match(first.context, /a: v1,\s+b: v2/);
  assert.doesNotThrow(() => parse(first.context));
});

test('cached contexts preserve public names and do not retain prior render edits', () => {
  const source = 'export const a = 1; function f(v1) { const b = a + v1; return { a, b, v1 }; }';
  const plan = select.planFile(source, { contextTokens: 100 });
  const outer = plan.stages[0][0].keys[0];
  for (const batch of plan.stages.flat()) {
    const baseline = select.renderBatch(source, batch);
    const renamed = select.renderBatch(source, batch, new Map([[outer, 'outerValue']]));
    assert.doesNotThrow(() => parse(renamed.context, { sourceType: 'module' }));
    assert.deepEqual(select.renderBatch(source, batch), baseline);
    if (batch === plan.stages[0][0]) assert.match(renamed.context, /v1 as a/);
    if (batch === plan.stages[1][0]) assert.match(renamed.context, /a: outerValue/);
  }
});

test('many real batches fill a local HTTP/2 session and persist valid rewrites', { timeout: 30_000 }, async t => {
  const root = await mkdtemp(join(tmpdir(), 'unminify-render-'));
  const codexHome = await mkdtemp(join(tmpdir(), 'unminify-render-auth-'));
  t.after(async () => {
    await rm(root, { recursive: true, force: true });
    await rm(codexHome, { recursive: true, force: true });
  });
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  const source = largeSource(80) + '\n' + Array.from({ length: 1600 }, (_, index) =>
    `function unchangedOperation${index}(inputValue) { return inputValue + ${index}; }`).join('\n');
  assert(Buffer.byteLength(source) > 100_000);
  await writeFile(join(root, 'large.js'), source);
  const token = `e30.${Buffer.from(JSON.stringify({ exp: Math.floor(Date.now() / 1000) + 3600 })).toString('base64url')}.local`;
  await writeFile(join(codexHome, 'auth.json'), JSON.stringify({ auth_mode: 'chatgpt',
    tokens: { access_token: token, account_id: 'local-render-test' } }));
  const concurrency = 20, pending = [], sessions = new Set();
  let received = 0, peak = 0;
  const server = createServer(async (req, res) => {
    let body = '';
    for await (const chunk of req) body += chunk;
    const request = JSON.parse(body);
    received++;
    const names = Object.fromEntries(Object.keys(request.text.format.schema.properties).map((key, index) =>
      [key, index === 0 ? 'inputValue' : 'computedValue']));
    pending.push(() => {
      res.writeHead(200, { 'content-type': 'text/event-stream' });
      res.end(`data: ${JSON.stringify({ type: 'response.completed', response: { status: 'completed',
        output: [{ type: 'message', role: 'assistant', content: [{ type: 'output_text', text: JSON.stringify(names) }] }],
        usage: { input_tokens: 12, output_tokens: 8 } } })}\n\n`);
    });
    peak = Math.max(peak, pending.length);
    if (pending.length === concurrency) pending.splice(0).forEach(finish => finish());
  });
  server.on('session', session => {
    sessions.add(session);
    session.on('error', () => {});
    session.on('close', () => sessions.delete(session));
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  t.after(async () => {
    for (const session of sessions) session.destroy();
    await new Promise(resolve => server.close(resolve));
  });
  const baseUrl = `http://127.0.0.1:${server.address().port}`;
  const summary = await run({ dirs: [root], codexHome, concurrency, contextTokens: 115 }, {
    ...select, ...rename, ...checks,
    createNamer: options => createNamer({ ...options, baseUrl, refreshUrl: `${baseUrl}/oauth/token`, timeoutMs: 5000 }),
    output: () => {},
  });
  assert.equal(summary.exitCode, 0);
  assert.equal(peak, concurrency);
  assert.equal(received, 80);
  const after = await readFile(join(root, 'large.js'), 'utf8');
  assert.match(after, /const computedValue = inputValue \+ 79/);
  assert.equal(checks.checkRewrite(source, after), null);
});

test('planning and packing a synthetic 150KB src file take seconds', () => {
  const source = largeSource(2400);
  assert(Buffer.byteLength(source) >= 150_000);
  for (const contextTokens of [100000, 2000]) {
    const started = performance.now();
    const { stages } = select.planFile(source, { contextTokens });
    const requests = stages.flatMap(stage => packBatches(stage,
      (batch, options) => select.renderBatch(source, batch, new Map(), options), { contextTokens }));
    const elapsed = performance.now() - started;
    assert(elapsed < 5000, `${elapsed}ms for planning and packing`);
    assert(requests.every(request => request.estimated_input_tokens <= contextTokens));
    assert.equal(requests.flatMap(request => request.parts.flatMap(part => part.batch.keys)).length, 4800);
    if (contextTokens === 100000) assert(requests.length <= 2);
  }
});
