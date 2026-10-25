import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer, constants } from 'node:http2';
import { mkdtemp, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createCodexClient } from '../scripts/codex-api-client.mjs';

// Upstream raw payload: openai/codex codex-api/src/sse/responses.rs,
// rate_limit_error_preserves_retry_delay (response created_at 1755041560).
const failed = JSON.parse(await readFile(new URL('./fixtures/responses-rate-limit.json', import.meta.url), 'utf8'));
const request = { model: 'local', effort: 'low', instructions: 'Return names.', input: [] };
const sse = event => `data: ${JSON.stringify(event)}\n\n`;

async function service(t, handler, options = {}) {
  const root = await mkdtemp(join(tmpdir(), 'stream-failure-'));
  const access = `e30.${Buffer.from(JSON.stringify({ exp: Date.now() / 1000 + 3600 })).toString('base64url')}.local`;
  await writeFile(join(root, 'auth.json'), JSON.stringify({ tokens: {
    access_token: access, account_id: 'private-account', refresh_token: 'private-refresh',
  } }));
  const server = createServer(async (req, res) => {
    for await (const chunk of req) { /* Drain the actual HTTP/2 request. */ }
    handler(req, res);
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const logs = [];
  const client = createCodexClient({ codexHome: root,
    baseUrl: `http://127.0.0.1:${server.address().port}`, retryDelayMs: 0,
    timeoutMs: 1000, log: row => logs.push(row), ...options });
  t.after(async () => {
    await client.close();
    await new Promise(resolve => server.close(resolve));
    await rm(root, { recursive: true, force: true });
  });
  return { client, logs, access };
}

test('failed SSE preserves the upstream error on every attempt and final rejection', async t => {
  const f = await service(t, (req, res) => {
    res.writeHead(200, { 'content-type': 'text/event-stream', 'x-request-id': 'req-local' });
    res.end(sse(failed));
  });
  await assert.rejects(f.client.respond(request), error => {
    assert.match(error.message, /rate_limit_exceeded/);
    assert.equal(error.failure.event_type, 'response.failed');
    return true;
  });
  assert.equal(f.logs.length, 4);
  for (const row of f.logs) {
    assert.equal(row.failure.code, failed.response.error.code);
    assert.equal(row.failure.message, failed.response.error.message);
    assert.equal(row.failure.response_id, failed.response.id);
    assert.equal(row.failure.response_status, 'failed');
    assert.equal(row.request_id, 'req-local');
    assert.equal(row.status, 200);
    assert.equal(row.stream.last_event, 'response.failed');
    assert.equal(row.stream.events, 1);
    assert(row.stream.bytes > 0);
  }
});

test('incomplete SSE retains its reason and usage and counts partial output without logging text', async t => {
  // Envelope follows upstream core/tests/suite/client.rs:
  // incomplete_response_emits_content_filter_error_message.
  const event = { type: 'response.incomplete', response: { id: 'resp_incomplete',
    object: 'response', status: 'incomplete', error: null,
    incomplete_details: { reason: 'content_filter' },
    usage: { input_tokens: 99000, output_tokens: 128000 } } };
  const f = await service(t, (req, res) => {
    res.writeHead(200);
    res.end(sse({ type: 'response.output_text.delta', delta: 'private output' }) + sse(event));
  });
  await assert.rejects(f.client.respond(request), /content_filter/);
  assert.equal(f.logs.length, 4);
  for (const row of f.logs) {
    assert.equal(row.failure.incomplete_reason, 'content_filter');
    assert.equal(row.failure.event_type, 'response.incomplete');
    assert.equal(row.usage.output_tokens, 128000);
    assert.equal(row.stream.output_text_chars, 14);
    assert.equal(row.stream.events, 2);
  }
  assert(!JSON.stringify(f.logs).includes('private output'));
});

test('HTTP error diagnostics preserve structured codes without retaining the response body', async t => {
  const f = await service(t, (req, res) => {
    res.writeHead(503);
    res.end(JSON.stringify({ error: { code: 'server_error', type: 'server_error',
      message: 'Unavailable private-refresh', param: null }, input: 'private source' }));
  });
  await assert.rejects(f.client.respond(request), /server_error/);
  assert.equal(f.logs.length, 4);
  assert(f.logs.every(row => row.failure.code === 'server_error' && row.status === 503));
  assert(!JSON.stringify(f.logs).includes('private-refresh'));
  assert(!JSON.stringify(f.logs).includes('private source'));
});

test('rate-limit attempts retain their structured failure before retrying successfully', async t => {
  let calls = 0;
  const f = await service(t, (req, res) => {
    if (++calls === 1) {
      res.writeHead(429, { 'retry-after': '0.001' });
      res.end(JSON.stringify({ error: failed.response.error }));
    } else {
      res.writeHead(200);
      res.end(sse({ type: 'response.completed', response: { status: 'completed',
        usage: { input_tokens: 12, output_tokens: 8 },
        output: [{ type: 'message', role: 'assistant', content: [{ type: 'output_text', text: 'hello' }] }] } }));
    }
  });
  assert.equal((await f.client.respond(request)).output, 'hello');
  assert.equal(f.logs.length, 2);
  assert.equal(f.logs[0].failure.code, 'rate_limit_exceeded');
  assert.equal(f.logs[1].failure, null);
});

test('idle timeout and peer resets retain distinct transport diagnostics', async t => {
  for (const mode of ['idle', 'reset']) {
    const f = await service(t, (req, res) => {
      res.writeHead(200);
      res.write(': heartbeat\n\n');
      if (mode === 'reset') {
        req.stream.on('error', () => {});
        req.stream.close(constants.NGHTTP2_INTERNAL_ERROR);
      }
    }, { timeoutMs: 30 });
    await assert.rejects(f.client.respond(request));
    assert.equal(f.logs.length, 4);
    for (const row of f.logs) {
      assert.equal(row.failure.source, 'transport');
      assert.equal(row.failure.code, mode === 'idle' ? 'REQUEST_TIMEOUT' : 'ERR_HTTP2_STREAM_ERROR');
      if (mode === 'reset') assert.equal(row.failure.rst_code, constants.NGHTTP2_INTERNAL_ERROR);
    }
  }
});

test('SSE diagnostics redact credentials, bound error messages and omit arbitrary fields', async t => {
  let access;
  const f = await service(t, (req, res) => {
    res.writeHead(200);
    res.end(sse({ ...failed, response: { ...failed.response, error: {
      code: 'server_error', message: `${access} private-refresh private-account ${'x'.repeat(5000)}`,
      source: 'private source',
    } } }));
  });
  access = f.access;
  await assert.rejects(f.client.respond(request), error => {
    for (const secret of [access, 'private-refresh', 'private-account']) assert(!String(error).includes(secret));
    return true;
  });
  for (const row of f.logs) {
    assert(row.failure.message.length <= 2048);
    assert(!JSON.stringify(row).includes('private source'));
    for (const secret of [access, 'private-refresh', 'private-account']) assert(!JSON.stringify(row).includes(secret));
  }
});
