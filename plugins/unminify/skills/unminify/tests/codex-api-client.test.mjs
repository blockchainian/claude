import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdtemp, writeFile, readFile, readdir, stat, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { createCodexClient } from '../scripts/codex-api-client.mjs';

const jwt = exp => `e30.${Buffer.from(JSON.stringify({ exp })).toString('base64url')}.fake`;
const fresh = () => jwt(Math.floor(Date.now() / 1000) + 3600);
const usage = { input_tokens: 12, cached_input_tokens: 4, output_tokens: 8, reasoning_output_tokens: 2 };
const input = [{ type: 'message', role: 'user', content: [{ type: 'input_text', text: 'hello' }] }];
const request = { model: 'test-model', effort: 'low', instructions: 'Return text.', input };

// Completion envelope follows codex-api/src/sse/responses.rs; text is local test data.
function completion(text = 'hello') {
  return { type: 'response.completed', response: { status: 'completed',
    output: [{ type: 'message', role: 'assistant', content: [{ type: 'output_text', text }] }],
    usage: { input_tokens: 12, input_tokens_details: { cached_tokens: 4 }, output_tokens: 8,
      output_tokens_details: { reasoning_tokens: 2 }, total_tokens: 20 } } };
}

async function fixture(t, handler, { expired = false } = {}) {
  const codexHome = await mkdtemp(join(tmpdir(), 'codex-client-test-'));
  const auth = { auth_mode: 'chatgpt', OPENAI_API_KEY: null, custom: { preserve: true },
    last_refresh: '2026-01-01T00:00:00Z', tokens: { id_token: 'fake-id',
      access_token: expired ? jwt(1) : fresh(), refresh_token: 'fake-refresh', account_id: 'fake-account', custom: 7 } };
  await writeFile(join(codexHome, 'auth.json'), JSON.stringify(auth), { mode: 0o600 });
  const server = createServer(async (req, res) => {
    try {
      let text = '';
      for await (const chunk of req) text += chunk;
      await handler(req, res, text ? JSON.parse(text) : null);
    } catch (error) { res.destroy(error); }
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const baseUrl = `http://127.0.0.1:${server.address().port}`;
  const logs = [];
  const options = { codexHome, baseUrl, refreshUrl: `${baseUrl}/oauth/token`,
    concurrency: 20, retryDelayMs: 1, timeoutMs: 2000, log: event => logs.push(event) };
  const clients = [];
  const client = extra => { const c = createCodexClient({ ...options, ...extra }); clients.push(c); return c; };
  t.after(async () => {
    await Promise.all(clients.map(c => c.close()));
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
    await rm(codexHome, { recursive: true, force: true });
  });
  return { client, auth, codexHome, logs, options };
}
function send(res, event = completion()) {
  res.writeHead(200, { 'content-type': 'text/event-stream', 'x-codex-primary-used-percent': '42',
    'x-ratelimit-secret': 'fake-refresh' });
  const data = `: heartbeat\r\nevent: ignored\r\ndata: ${JSON.stringify(event)}\r\n\r\ndata: [DONE]\r\n\r\n`;
  res.write(data.slice(0, 19)); res.end(data.slice(19));
}

test('SSE completion, usage, request shape and safe per-attempt metrics', async t => {
  const f = await fixture(t, (req, res, body) => {
    assert.equal(req.url, '/responses');
    assert.equal(req.headers.originator, 'codex_cli_rs');
    assert.equal(req.headers['chatgpt-account-id'], 'fake-account');
    assert.equal(body.model, request.model); assert.deepEqual(body.reasoning, { effort: 'low' });
    assert.deepEqual(body.input, input); assert.deepEqual(body.tools, []);
    assert.equal(body.store, false); assert.equal(body.stream, true);
    send(res);
  });
  assert.deepEqual(await f.client().respond(request), { output: 'hello', usage });
  assert.equal(f.logs.length, 1); assert.equal(f.logs[0].status, 200);
  assert.deepEqual(f.logs[0].usage, usage);
  assert(f.logs[0].duration_ms >= 0);
  assert.equal(f.logs[0].rate_headers['x-codex-primary-used-percent'], '42');
  const serialized = JSON.stringify(f.logs);
  for (const secret of Object.values(f.auth.tokens).filter(v => typeof v === 'string')) assert(!serialized.includes(secret));
});

test('SSE item events, multiline data and split UTF-8 bytes', async t => {
  const f = await fixture(t, async (req, res) => {
    const complete = completion(); delete complete.response.output;
    const text = `data: ${JSON.stringify({ type: 'response.output_item.done', output_index: 0,
      item: { type: 'message', role: 'assistant', content: [{ type: 'output_text', text: '你好' }] } })}\n\n`
      + `data: ${JSON.stringify(complete, null, 2).replaceAll('\n', '\ndata: ')}\n\n`;
    const bytes = Buffer.from(text);
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    const split = bytes.indexOf(Buffer.from('你好')) + 1;
    res.write(bytes.subarray(0, split)); await delay(5); res.end(bytes.subarray(split));
  });
  assert.deepEqual(await f.client().respond(request), { output: '你好', usage });
});

test('temporary rate limits retry rather than terminally exhausting the client', async t => {
  let calls = 0;
  const f = await fixture(t, (req, res) => {
    calls++;
    if (calls === 1) { res.writeHead(429); res.end(JSON.stringify({ error: { code: 'rate_limit_exceeded' } })); }
    else send(res);
  });
  const c = f.client();
  await c.respond(request); await c.respond(request);
  assert.equal(calls, 3);
});

test('optional schema parses JSON and retries rejected asynchronous validation', async t => {
  let calls = 0;
  const f = await fixture(t, (req, res) => { calls++; send(res, completion(JSON.stringify({ count: calls }))); });
  const result = await f.client().respond({ ...request, schema: { type: 'object' },
    async validate(value) { await delay(1); if (value.count === 1) throw new Error('invalid'); } });
  assert.deepEqual(result.output, { count: 2 });
  assert.equal(calls, 2);
});

test('401 refresh rotates credentials, preserves fields, replaces inode with private file', async t => {
  let models = 0, refreshes = 0;
  const token = fresh() + 'rotated';
  const f = await fixture(t, (req, res, body) => {
    if (req.url === '/oauth/token') {
      refreshes++;
      assert.deepEqual(body, { client_id: 'app_EMoamEEZ73f0CkXaXp7hrann', grant_type: 'refresh_token',
        refresh_token: 'fake-refresh', refresh_reason: 'unauthorized_recovery' });
      res.end(JSON.stringify({ access_token: token, refresh_token: 'rotated-refresh', id_token: 'rotated-id' }));
    } else {
      models++;
      if (req.headers.authorization !== `Bearer ${token}`) { res.writeHead(401); res.end('private error'); }
      else send(res);
    }
  });
  const before = await stat(join(f.codexHome, 'auth.json'));
  await f.client().respond(request);
  const after = await stat(join(f.codexHome, 'auth.json'));
  assert.notEqual(before.ino, after.ino); assert.equal(after.mode & 0o777, 0o600);
  const saved = JSON.parse(await readFile(join(f.codexHome, 'auth.json'), 'utf8'));
  assert.deepEqual(saved, { ...f.auth, last_refresh: saved.last_refresh,
    tokens: { ...f.auth.tokens, access_token: token, refresh_token: 'rotated-refresh', id_token: 'rotated-id' } });
  assert(Date.parse(saved.last_refresh) > Date.parse(f.auth.last_refresh));
  assert.deepEqual(await readdir(f.codexHome), ['auth.json']);
  assert.equal(models, 2); assert.equal(refreshes, 1);
});

test('20 concurrent calls across clients share one proactive refresh', async t => {
  let refreshes = 0, models = 0;
  const f = await fixture(t, async (req, res) => {
    if (req.url === '/oauth/token') {
      refreshes++; await delay(30);
      res.end(JSON.stringify({ access_token: fresh(), refresh_token: 'rotated-refresh' }));
    } else { models++; send(res); }
  }, { expired: true });
  const a = f.client(), b = f.client();
  await Promise.all(Array.from({ length: 20 }, (_, i) => (i % 2 ? a : b).respond(request)));
  assert.equal(refreshes, 1); assert.equal(models, 20);
});

test('independent processes coordinate refresh through the auth file lock', async t => {
  let refreshes = 0, models = 0;
  const f = await fixture(t, async (req, res) => {
    if (req.url === '/oauth/token') {
      refreshes++; await delay(60);
      res.end(JSON.stringify({ access_token: fresh(), refresh_token: 'rotated-refresh' }));
    } else { models++; send(res); }
  }, { expired: true });
  const clientModule = new URL('../scripts/codex-api-client.mjs', import.meta.url).href;
  const code = `import { createCodexClient } from ${JSON.stringify(clientModule)};
    const client = createCodexClient(JSON.parse(process.argv[1]));
    try { await client.respond(${JSON.stringify(request)}); } finally { await client.close(); }`;
  // The children receive only this fixture's private directory and loopback URLs.
  const run = promisify(execFile);
  const options = JSON.stringify({ codexHome: f.codexHome, baseUrl: f.options.baseUrl,
    refreshUrl: f.options.refreshUrl, timeoutMs: 2000 });
  await Promise.all([0, 1].map(() => run(process.execPath, ['--input-type=module', '-e', code, options], { timeout: 5000 })));
  assert.equal(refreshes, 1); assert.equal(models, 2);
});

test('concurrent 401 recovery refreshes once', async t => {
  let refreshes = 0;
  const token = fresh() + 'rotated';
  const f = await fixture(t, async (req, res) => {
    if (req.url === '/oauth/token') { refreshes++; await delay(20); res.end(JSON.stringify({ access_token: token })); }
    else if (req.headers.authorization !== `Bearer ${token}`) { await delay(10); res.writeHead(401); res.end(); }
    else send(res);
  });
  const client = f.client();
  await Promise.all(Array.from({ length: 20 }, () => client.respond(request)));
  assert.equal(refreshes, 1);
});

test('usage exhaustion stops queued and subsequent requests', async t => {
  let calls = 0;
  const f = await fixture(t, (req, res) => {
    calls++; res.writeHead(429); res.end(JSON.stringify({ error: { type: 'usage_limit_reached' } }));
  });
  const c = f.client({ concurrency: 1 });
  const results = await Promise.allSettled(Array.from({ length: 20 }, () => c.respond(request)));
  assert(results.every(r => r.status === 'rejected' && r.reason.code === 'USAGE_LIMIT'));
  await assert.rejects(c.respond(request), { code: 'USAGE_LIMIT' });
  assert.equal(calls, 1);
});

test('quota errors in SSE stop further requests', async t => {
  let calls = 0;
  const f = await fixture(t, (req, res) => { calls++; send(res, { type: 'response.failed',
    response: { error: { code: 'credit_balance_exhausted' } } }); });
  const c = f.client();
  await assert.rejects(c.respond(request), { code: 'USAGE_LIMIT' });
  await assert.rejects(c.respond(request), { code: 'USAGE_LIMIT' });
  assert.equal(calls, 1);
});

test('concurrency remains bounded through full SSE consumption; close drains calls', async t => {
  let active = 0, peak = 0;
  const f = await fixture(t, async (req, res) => {
    active++; peak = Math.max(peak, active); await delay(15); active--; send(res);
  });
  const c = f.client({ concurrency: 3 });
  const pending = Array.from({ length: 20 }, () => c.respond(request));
  await c.close(); await Promise.all(pending);
  assert.equal(peak, 3);
  await assert.rejects(c.respond(request));
});

test('network errors, 5xx, broken streams and timeouts retry three times then fail safely', async t => {
  for (const mode of ['network', 'http', 'stream', 'timeout']) {
    let calls = 0;
    const f = await fixture(t, (req, res) => {
      calls++;
      if (mode === 'network') req.socket.destroy();
      if (mode === 'http') { res.writeHead(503); res.end('fake-refresh'); }
      if (mode === 'stream') res.end('data: {broken}\n\n');
      if (mode === 'timeout') { res.writeHead(200); res.write(': heartbeat\n\n'); }
    });
    await assert.rejects(f.client({ timeoutMs: 30 }).respond(request), error => {
      assert.equal(error.code, undefined); assert(!String(error).includes('fake-refresh')); return true;
    });
    assert.equal(calls, 4);
    assert.equal(f.logs.length, 4);
  }
});

test('refresh failure is terminal AUTH with no credential material', async t => {
  let calls = 0;
  const f = await fixture(t, (req, res) => { calls++; res.writeHead(400); res.end('fake-refresh'); }, { expired: true });
  const c = f.client();
  await assert.rejects(c.respond(request), error => error.code === 'AUTH' && !String(error).includes('fake-refresh'));
  await assert.rejects(c.respond(request), { code: 'AUTH' });
  assert.equal(calls, 1);
});
