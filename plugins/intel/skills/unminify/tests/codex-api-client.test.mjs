import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http2';
import { mkdtemp, writeFile, readFile, readdir, stat, rm, rename, utimes } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { execFile, spawn } from 'node:child_process';
import { once } from 'node:events';
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
  const sessions = new Set();
  server.on('session', session => {
    sessions.add(session);
    session.on('error', () => {});
    session.on('close', () => sessions.delete(session));
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
    for (const session of sessions) session.destroy();
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

function clientArgs(f) {
  const clientModule = new URL('../scripts/codex-api-client.mjs', import.meta.url).href;
  const code = `import { createCodexClient } from ${JSON.stringify(clientModule)};
    const client = createCodexClient(JSON.parse(process.argv[1]));
    try { await client.respond(${JSON.stringify(request)}); } finally { await client.close(); }`;
  // Children receive only this fixture's private directory and loopback URLs.
  const options = JSON.stringify({ codexHome: f.codexHome, baseUrl: f.options.baseUrl,
    refreshUrl: f.options.refreshUrl, timeoutMs: 2000 });
  return ['--input-type=module', '-e', code, options];
}

test('HTTP/2 multiplexes concurrent requests and refresh on one origin session; close releases it', async t => {
  const sessions = new Set();
  let active = 0, peak = 0, refreshes = 0;
  const f = await fixture(t, async (req, res) => {
    assert.equal(req.httpVersion, '2.0');
    sessions.add(req.stream.session);
    if (req.url === '/oauth/token') {
      refreshes++;
      res.end(JSON.stringify({ access_token: fresh() }));
    } else {
      active++; peak = Math.max(peak, active);
      await delay(20); active--; send(res);
    }
  }, { expired: true });
  const c = f.client();
  await Promise.all(Array.from({ length: 20 }, () => c.respond(request)));
  assert.equal(refreshes, 1);
  assert.equal(peak, 20);
  assert.equal(sessions.size, 1);
  const [session] = sessions;
  const closed = once(session, 'close');
  await c.close();
  await closed;
  await c.close();
});

test('GOAWAY and disconnected sessions are replaced for subsequent requests', { timeout: 3000 }, async t => {
  const sessions = new Set();
  let current;
  const f = await fixture(t, (req, res) => {
    current = req.stream.session;
    sessions.add(current);
    send(res);
  });
  const c = f.client();
  await c.respond(request);
  current.goaway();
  // The following request starts after the peer has received GOAWAY.
  await delay(20);
  await c.respond(request);
  assert.equal(sessions.size, 2);
  const closed = once(current, 'close');
  current.destroy();
  await closed;
  await c.respond(request);
  assert.equal(sessions.size, 3);
  assert.deepEqual(f.logs.map(row => row.session_id), [1, 2, 3]);
});

test('refresh on another origin uses a separate HTTP/2 session', async t => {
  let refreshSession, modelSession;
  const authServer = await fixture(t, (req, res) => {
    assert.equal(req.httpVersion, '2.0');
    refreshSession = req.stream.session;
    res.end(JSON.stringify({ access_token: fresh() }));
  });
  const f = await fixture(t, (req, res) => { modelSession = req.stream.session; send(res); }, { expired: true });
  await f.client({ refreshUrl: authServer.options.refreshUrl }).respond(request);
  assert(refreshSession);
  assert(modelSession);
  assert.notEqual(refreshSession, modelSession);
});

test('a timed out stream leaves concurrent requests and its shared session usable', async t => {
  const sessions = new Set();
  const f = await fixture(t, (req, res, body) => {
    sessions.add(req.stream.session);
    if (body.instructions === 'stall') {
      res.writeHead(200); res.write(': heartbeat\n\n');
    } else send(res);
  });
  const c = f.client({ timeoutMs: 30 });
  const results = await Promise.allSettled([c.respond({ ...request, instructions: 'stall' }), c.respond(request)]);
  assert.equal(results[0].status, 'rejected');
  assert.equal(results[1].status, 'fulfilled');
  assert.deepEqual(await c.respond(request), { output: 'hello', usage });
  assert.equal(sessions.size, 1);
  const attempts = f.logs.filter(event => event.usage === null);
  assert.equal(attempts.length, 4);
  assert(attempts.every(event => event.status === 200 && event.duration_ms >= 20));
});

test('a response that keeps streaming outlives the timeout, which measures only idle time', async t => {
  const f = await fixture(t, async (req, res) => {
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    for (let i = 0; i < 8; i++) { res.write(': heartbeat\n\n'); await delay(20); }
    res.end(`data: ${JSON.stringify(completion())}\n\ndata: [DONE]\n\n`);
  });
  assert.deepEqual(await f.client({ timeoutMs: 80 }).respond(request), { output: 'hello', usage });
  assert.equal(f.logs.length, 1);
});

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

test('redirects are refused and retried as transport failures without following them', async t => {
  let calls = 0;
  const f = await fixture(t, (req, res) => {
    assert.equal(req.url, '/responses');
    calls++;
    res.writeHead(307, { location: '/redirected' }); res.end();
  });
  await assert.rejects(f.client().respond(request), /network or stream failure/);
  assert.equal(calls, 4);
  assert(f.logs.every(event => event.status === 307));
});

test('temporary rate limits retry rather than terminally exhausting the client', async t => {
  let calls = 0;
  const f = await fixture(t, (req, res) => {
    calls++;
    if (calls === 1) { res.writeHead(429); res.end(JSON.stringify({ error: { code: 'rate_limit_exceeded' } })); }
    else send(res);
  });
  const c = f.client({ rateLimitDelayMs: 20 });
  await c.respond(request); await c.respond(request);
  assert.equal(calls, 3);
});

test('429 pauses all concurrent and queued calls, extends the shared deadline and preserves retries', async t => {
  const attempts = new Map(), arrivals = [], pauses = [];
  let initial = 0, release;
  const ready = new Promise(resolve => { release = resolve; });
  const f = await fixture(t, async (req, res, body) => {
    const id = Number(body.instructions);
    const attempt = (attempts.get(id) ?? 0) + 1;
    attempts.set(id, attempt);
    arrivals.push({ id, attempt, at: performance.now() });
    if (attempt === 1 && id < 3) {
      if (++initial === 3) release();
      await ready;
      if (id === 1) await delay(30);
      if (id === 2) { await delay(45); return send(res); }
    }
    if (id < 2 && attempt <= 5 && !(id === 0 && attempt === 2)) {
      const seconds = id === 1 && attempt === 1 ? 0.09 : 0.06;
      const start = performance.now();
      pauses.push({ start, end: start + seconds * 1000 });
      res.writeHead(429, { 'retry-after': String(seconds) });
      res.flushHeaders();
      // Pausing starts at the headers, even if the error body arrives later.
      if (id === 0 && attempt === 1) await delay(50);
      return res.end(JSON.stringify({ detail: 'Rate limit exceeded' }));
    }
    if (id === 0 && [2, 6, 7].includes(attempt)) {
      res.writeHead(503); return res.end();
    }
    send(res);
  });
  const c = f.client({ concurrency: 3 });
  const results = await Promise.all(Array.from({ length: 12 }, (_, id) =>
    c.respond({ ...request, instructions: String(id) })));
  assert(results.every(result => result.output === 'hello'));
  assert.equal(attempts.get(0), 8);
  assert.equal(attempts.get(1), 6);
  // All initial streams arrived before these two pauses; later waves can have
  // streams already in flight when a peer's 429 reaches the server.
  const violations = arrivals.filter(({ id, attempt, at }) => !(id < 3 && attempt === 1)
    && pauses.slice(0, 2).some(({ start, end }) => at >= start && at < end));
  assert.deepEqual(violations, [], 'no new requests reach the server during any shared pause');
  assert.equal(f.logs.filter(event => event.status === 429).length, 9);
  assert(f.logs.filter(event => event.status === 429).every(event => event.rate_headers['retry-after']));
  assert.equal(f.logs.length, arrivals.length);
});

test('missing or invalid retry-after uses the configured default pause', async t => {
  for (const header of [undefined, 'invalid']) {
    const arrivals = [];
    let pausedAt;
    const f = await fixture(t, (req, res) => {
      arrivals.push(performance.now());
      if (arrivals.length === 1) {
        pausedAt = performance.now();
        res.writeHead(429, header === undefined ? {} : { 'retry-after': header });
        res.end(JSON.stringify({ detail: 'Rate limit exceeded' }));
      } else send(res);
    });
    const c = f.client({ concurrency: 1, rateLimitDelayMs: 40 });
    const results = await Promise.all([c.respond(request), c.respond(request)]);
    assert(results.every(result => result.output === 'hello'));
    assert.equal(arrivals.length, 3);
    assert(arrivals.slice(1).every(at => at >= pausedAt + 40));
    assert.equal(f.logs.filter(event => event.status === 429).length, 1);
  }
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
  const run = promisify(execFile);
  await Promise.all([0, 1].map(() => run(process.execPath, clientArgs(f), { timeout: 5000 })));
  assert.equal(refreshes, 1); assert.equal(models, 2);
});

test('SIGKILL during refresh leaves a lock that the next process reclaims', { timeout: 5000 }, async t => {
  let refreshes = 0, ready;
  const started = new Promise(resolve => { ready = resolve; });
  const f = await fixture(t, (req, res) => {
    if (req.url !== '/oauth/token') return send(res);
    refreshes++;
    if (refreshes === 1) ready();
    else res.end(JSON.stringify({ access_token: fresh(), refresh_token: 'rotated-refresh' }));
  }, { expired: true });
  const args = clientArgs(f);
  const child = spawn(process.execPath, args, { stdio: 'ignore' });
  t.after(() => { if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL'); });
  await started;
  const lockPath = join(f.codexHome, 'auth.json.refresh.lock');
  const owner = JSON.parse(await readFile(lockPath, 'utf8'));
  assert.equal(owner.pid, child.pid);
  const exited = once(child, 'exit');
  child.kill('SIGKILL');
  assert.deepEqual(await exited, [null, 'SIGKILL']);
  assert.throws(() => process.kill(owner.pid, 0), { code: 'ESRCH' });
  await promisify(execFile)(process.execPath, args, { timeout: 1500 });
  assert(Number.isFinite(owner.createdAt));
  assert.equal(refreshes, 2);
  assert.deepEqual(await readdir(f.codexHome), ['auth.json']);
});

test('an expired lock is reclaimed even when its recorded process is alive', async t => {
  let refreshes = 0;
  const f = await fixture(t, (req, res) => {
    if (req.url === '/oauth/token') {
      refreshes++; res.end(JSON.stringify({ access_token: fresh() }));
    } else send(res);
  }, { expired: true });
  await writeFile(join(f.codexHome, 'auth.json.refresh.lock'),
    JSON.stringify({ pid: process.pid, createdAt: Date.now() - 121_000 }));
  await Promise.all(Array.from({ length: 4 }, () => promisify(execFile)(process.execPath,
    clientArgs(f), { timeout: 1500 })));
  assert.equal(refreshes, 1);
  assert.deepEqual(await readdir(f.codexHome), ['auth.json']);
});

test('an expired partially written lock is reclaimed using its file timestamp', async t => {
  const f = await fixture(t, (req, res) => {
    if (req.url === '/oauth/token') res.end(JSON.stringify({ access_token: fresh() }));
    else send(res);
  }, { expired: true });
  const lockPath = join(f.codexHome, 'auth.json.refresh.lock');
  await writeFile(lockPath, '{');
  const staleTime = new Date(Date.now() - 121_000);
  await utimes(lockPath, staleTime, staleTime);
  await promisify(execFile)(process.execPath, clientArgs(f), { timeout: 1500 });
  assert.deepEqual(await readdir(f.codexHome), ['auth.json']);
});

test('a replaced lease prevents auth writes and preserves the successor lock on release', async t => {
  let lockPath;
  const successor = { pid: process.pid, createdAt: Date.now() };
  const f = await fixture(t, async (req, res) => {
    assert.equal(req.url, '/oauth/token');
    await writeFile(`${lockPath}.successor`, JSON.stringify(successor));
    await rename(`${lockPath}.successor`, lockPath);
    res.end(JSON.stringify({ access_token: fresh(), refresh_token: 'rotated-refresh' }));
  }, { expired: true });
  lockPath = join(f.codexHome, 'auth.json.refresh.lock');
  await assert.rejects(f.client().respond(request), { code: 'AUTH' });
  assert.deepEqual(JSON.parse(await readFile(lockPath, 'utf8')), successor);
  assert.deepEqual(JSON.parse(await readFile(join(f.codexHome, 'auth.json'), 'utf8')), f.auth);
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

test('usage exhaustion immediately wakes calls waiting on the shared rate limit', { timeout: 1000 }, async t => {
  let limited;
  const paused = new Promise(resolve => { limited = resolve; });
  let calls = 0;
  const f = await fixture(t, async (req, res, body) => {
    calls++;
    if (body.instructions === 'quota') {
      await paused;
      res.writeHead(429, { 'retry-after': '39' });
      res.end(JSON.stringify({ error: { type: 'usage_limit_reached' } }));
    } else {
      res.writeHead(429, { 'retry-after': '39' });
      res.end(JSON.stringify({ detail: 'Rate limit exceeded' }));
    }
  });
  const c = f.client({ concurrency: 2, log: event => {
    f.logs.push(event);
    if (event.status === 429) limited();
  } });
  const results = await Promise.allSettled([c.respond(request),
    c.respond({ ...request, instructions: 'quota' }), c.respond(request)]);
  assert(results.every(result => result.status === 'rejected' && result.reason.code === 'USAGE_LIMIT'));
  await assert.rejects(c.respond(request), { code: 'USAGE_LIMIT' });
  assert.equal(calls, 2);
  assert.equal(f.logs.length, 2);
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
  for (const mode of ['network', 'http', 'stream', 'timeout', 'headers-timeout']) {
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

test('attempt telemetry measures queue, headers, retry, pause and refresh without payloads', async t => {
  let calls = 0;
  const events = [];
  const f = await fixture(t, async (req, res) => {
    if (req.url === '/oauth/token') { res.end(JSON.stringify({ access_token: fresh() })); return; }
    calls++;
    await delay(10);
    if (calls === 1) { res.writeHead(429, { 'retry-after': '0.03' }); res.end('{}'); return; }
    send(res);
  }, { expired: true });
  const c = f.client({ concurrency: 1, runId: 'telemetry', event: record => events.push(record) });
  await Promise.all(Array.from({ length: 3 }, (_, i) => c.respond({ ...request,
    telemetry: { file: 'a.js', batch_id: String(i), phase: 'model' } })));
  await c.close();
  assert.equal(f.logs.length, 5);
  for (const row of f.logs) {
    assert.equal(row.run_id, 'telemetry');
    assert(row.queued_at <= row.sent_at);
    assert(row.sent_at <= row.headers_at);
    assert(row.headers_at <= row.ended_at);
    assert.equal(row.duration_ms, row.ended_at - row.sent_at);
    assert(row.body_bytes > 0);
    assert.equal(row.in_flight, 1);
    assert(row.queued >= 0);
    assert.equal(row.session_id, 1);
    assert(row.attempt >= 1);
  }
  const model = f.logs.filter(row => row.kind === 'model');
  assert(model.some(row => row.queued > 0));
  assert(model.some(row => row.sent_at - row.queued_at >= 20));
  assert.equal(model[1].attempt, 2);
  assert.equal(model[1].previous_failure, 'HTTP_429');
  assert.equal(model[0].file, 'a.js');
  assert.equal(model[0].batch_id, '0');
  assert.equal(model[0].phase, 'model');
  const start = events.find(row => row.type === 'pause_start');
  const end = events.find(row => row.type === 'pause_end');
  assert.equal(start.retry_after, '0.03');
  assert.equal(end.pause_id, start.pause_id);
  assert(end.ended_at - start.started_at >= 25);
  const serialized = JSON.stringify([f.logs, events]);
  for (const secret of [...Object.values(f.auth.tokens).filter(v => typeof v === 'string'), 'hello', 'Return text.'])
    assert(!serialized.includes(secret));
});
