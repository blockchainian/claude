// Reusable direct Codex Responses client backed by a ChatGPT file login.
import { readFile, open, rename, unlink, stat } from 'node:fs/promises';
import { homedir } from 'node:os';
import { resolve, join } from 'node:path';
import { connect, constants } from 'node:http2';
import { randomUUID } from 'node:crypto';
import { setTimeout as delay } from 'node:timers/promises';

const refreshes = new Map();
const CLIENT_ID = 'app_EMoamEEZ73f0CkXaXp7hrann';
const quotaCodes = new Set(['usage_limit_reached', 'usage_not_included', 'insufficient_quota',
  'credit_balance_exhausted', 'organization_spend_limit_exceeded', 'project_spend_limit_exceeded',
  'organization_usage_limit_exceeded', 'workspace_member_usage_limit_reached']);
const authError = () => Object.assign(new Error('Codex authentication failed; run codex login.'), { code: 'AUTH' });
const quotaError = () => Object.assign(new Error('Codex usage limit reached.'), { code: 'USAGE_LIMIT' });
const retryError = message => Object.assign(new Error(message), { retryable: true });
const hasQuotaError = error => error && [error.type, error.code].some(code => quotaCodes.has(code));
const number = value => Number.isSafeInteger(value) && value >= 0 ? value : null;

function usageOf(usage) {
  if (!usage) return null;
  return { input_tokens: number(usage.input_tokens), cached_input_tokens: number(usage.input_tokens_details?.cached_tokens),
    output_tokens: number(usage.output_tokens), reasoning_output_tokens: number(usage.output_tokens_details?.reasoning_tokens) };
}

function rateHeaders(headers, secrets) {
  const selected = {};
  for (const [key, value] of headers) {
    if (key === 'retry-after' || /^x-ratelimit-/.test(key) || /^ratelimit(?:-|$)/.test(key)
      || /^x-[a-z0-9-]+-(?:primary|secondary)-(?:used-percent|window-minutes|reset-at)$/.test(key)
      || /^x-codex-credits-(?:has-credits|unlimited|balance)$/.test(key)) {
      if ((/^[0-9.,:;=+% /TtZz-]+$/.test(value) || ['true', 'false'].includes(value))
        && ![...secrets].some(secret => value.includes(secret))) selected[key] = value;
    }
  }
  return selected;
}

function refreshReason(auth) {
  try {
    const exp = JSON.parse(Buffer.from(auth.tokens.access_token.split('.')[1], 'base64url')).exp;
    if (Number.isFinite(exp)) return exp * 1000 <= Date.now() + 300_000 ? 'proactive_expiry' : null;
  } catch { /* Opaque tokens use Codex's scheduled refresh policy. */ }
  const refreshed = Date.parse(auth.last_refresh);
  if (!Number.isFinite(refreshed)) throw authError();
  return refreshed < Date.now() - 8 * 86_400_000 ? 'scheduled' : null;
}

async function consume(response, metrics) {
  let buffer = '', completed = false;
  let output;
  const items = new Map();
  const decoder = new TextDecoder();
  function event(block) {
    const data = block.split(/\r?\n/).filter(line => line.startsWith('data:'))
      .map(line => line.slice(5).replace(/^ /, '')).join('\n');
    if (!data || data === '[DONE]') return;
    let e;
    try { e = JSON.parse(data); } catch { throw retryError('Invalid Responses SSE.'); }
    if (e.response?.usage) metrics.usage = usageOf(e.response.usage);
    if (hasQuotaError(e.error) || hasQuotaError(e.response?.error) || hasQuotaError(e)) throw quotaError();
    if (['error', 'response.failed', 'response.incomplete'].includes(e.type)) throw retryError('Responses stream failed.');
    if (e.type === 'response.output_item.done') items.set(e.output_index ?? e.item?.id ?? items.size, e.item);
    if (e.type === 'response.completed') {
      if (e.response?.status && e.response.status !== 'completed') throw retryError('Responses stream did not complete.');
      completed = true;
      if (Array.isArray(e.response?.output)) output = e.response.output;
    }
  }
  for await (const chunk of response.body) {
    buffer += decoder.decode(chunk, { stream: true });
    let match;
    while ((match = /\r?\n\r?\n/.exec(buffer))) {
      if (match.index > 2_000_000) throw retryError('Responses SSE event too large.');
      event(buffer.slice(0, match.index)); buffer = buffer.slice(match.index + match[0].length);
    }
    if (buffer.length > 2_000_000) throw retryError('Responses SSE event too large.');
  }
  buffer += decoder.decode();
  if (buffer.trim()) event(buffer);
  if (!completed || metrics.usage?.input_tokens == null || metrics.usage?.output_tokens == null)
    throw retryError('Missing Responses completion or usage.');
  let text = '';
  for (const item of output ?? [...items.values()]) {
    if (item?.type === 'reasoning') continue;
    if (item?.type !== 'message' || item.role !== 'assistant' || !Array.isArray(item.content))
      throw retryError('Unexpected Responses output.');
    for (const part of item.content) {
      if (part.type !== 'output_text' || typeof part.text !== 'string') throw retryError('Unexpected Responses content.');
      text += part.text;
    }
  }
  if (!text) throw retryError('Missing Responses output.');
  return text;
}

/** A file-backed ChatGPT Responses client. Endpoint and timing overrides support local tests.
 * respond({ model, effort, instructions, input, schema?, validate? }) returns { output, usage }.
 * With schema, output is parsed JSON; validate(output) may throw to request a retry.
 * log receives timing, queue, session and safe response metadata for each HTTP attempt.
 * event receives shared rate-limit pause boundaries; telemetry identifies the calling batch.
 * close() drains accepted calls and prevents further calls.
 */
export function createCodexClient({ concurrency = 20, codexHome = process.env.CODEX_HOME || join(homedir(), '.codex'),
  log = () => {}, event = () => {}, queued = () => 0, runId = randomUUID(), baseUrl = 'https://chatgpt.com/backend-api/codex',
  refreshUrl = 'https://auth.openai.com/oauth/token', timeoutMs = 120_000, retryDelayMs = 500,
  rateLimitDelayMs = 60_000 } = {}) {
  if (!Number.isSafeInteger(concurrency) || concurrency < 1) throw new Error('Invalid concurrency.');
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0 || !Number.isFinite(retryDelayMs) || retryDelayMs < 0
    || !Number.isFinite(rateLimitDelayMs) || rateLimitDelayMs <= 0)
    throw new Error('Invalid request timing.');
  const authPath = resolve(codexHome, 'auth.json');
  const endpoint = `${baseUrl.replace(/\/$/, '')}/responses`;
  const secrets = new Set();
  let terminal, accepting = true, active = 0;
  const queue = [], drain = [];
  const sessions = new Map(), connections = new Set();
  let pausedUntil = 0, pauseWait, inFlight = 0, sessionNumber = 0, pauseNumber = 0;
  const sessionIds = new WeakMap(), pauses = new Map();
  function endPauses() {
    const ended_at = Date.now();
    for (const [pause_id, started_at] of pauses) event({ type: 'pause_end', run_id: runId, pause_id, started_at, ended_at, at: ended_at });
    pauses.clear();
  }
  const stopped = new AbortController();

  function stop(error) {
    terminal = error;
    stopped.abort();
    endPauses();
    return error;
  }
  async function waitForRateLimit() {
    if (terminal) throw terminal;
    if (performance.now() >= pausedUntil) { endPauses(); return; }
    if (!pauseWait) pauseWait = (async () => {
      while (performance.now() < pausedUntil) {
        await delay(Math.min(pausedUntil - performance.now(), 2_147_483_647), undefined,
          { signal: stopped.signal });
      }
    })().catch(error => { throw terminal ?? error; }).finally(() => { pauseWait = undefined; endPauses(); });
    await pauseWait;
  }

  function remember(auth) {
    for (const value of [auth.OPENAI_API_KEY, ...Object.values(auth.tokens ?? {})])
      if (typeof value === 'string' && value) secrets.add(value);
  }
  async function readAuth() {
    try {
      const auth = JSON.parse(await readFile(authPath, 'utf8'));
      remember(auth);
      if (auth.auth_mode !== undefined && auth.auth_mode !== 'chatgpt') throw authError();
      const tokens = auth.tokens;
      if (!tokens || typeof tokens.access_token !== 'string' || !tokens.access_token
        || typeof tokens.account_id !== 'string' || !tokens.account_id) throw authError();
      return auth;
    } catch { throw authError(); }
  }

  async function logged(kind, operation, context = {}) {
    const metrics = { kind, run_id: runId, file: context.file ?? null, batch_id: context.batch_id ?? null,
      estimated_input_tokens: context.estimated_input_tokens ?? null,
      batch_ids: context.batch_ids ?? null, name_count: context.name_count ?? null,
      phase: context.phase ?? 'auth', queued_at: context.queued_at ?? Date.now(), sent_at: null,
      headers_at: null, ended_at: null, attempt: context.attempt ?? 1,
      previous_failure: context.previous_failure ?? null, session_id: null, body_bytes: 0,
      in_flight: 0, queued: 0, status: null, duration_ms: 0, usage: null, rate_headers: {} };
    try { return await operation(metrics); }
    finally {
      metrics.ended_at = Date.now();
      metrics.duration_ms = metrics.sent_at === null ? 0 : metrics.ended_at - metrics.sent_at;
      if (metrics.sent_at !== null) inFlight--;
      try { await log(metrics); } catch { throw new Error('Request logging failed.'); }
    }
  }
  function sessionFor(origin) {
    let session = sessions.get(origin);
    if (session && !session.closed && !session.destroyed) return session;
    session = connect(origin);
    sessionIds.set(session, ++sessionNumber);
    sessions.set(origin, session);
    connections.add(session);
    const forget = () => { if (sessions.get(origin) === session) sessions.delete(origin); };
    session.on('goaway', () => { forget(); session.close(); });
    session.on('error', () => { forget(); session.destroy(); });
    session.on('close', () => { forget(); connections.delete(session); });
    return session;
  }

  async function send(url, body, metrics, headers = {}) {
    const payload = JSON.stringify(body);
    const target = new URL(url);
    const session = sessionFor(target.origin);
    metrics.body_bytes = Buffer.byteLength(payload);
    metrics.session_id = sessionIds.get(session);
    return new Promise((resolve, reject) => {
      const stream = session.request({ ':method': 'POST', ':path': target.pathname + target.search,
        'content-type': 'application/json', ...headers,
        [constants.sensitiveHeaders]: ['authorization', 'chatgpt-account-id'] });
      metrics.sent_at = Date.now();
      metrics.in_flight = ++inFlight;
      metrics.queued = queued() + queue.length + Math.max(0, active - inFlight);
      // Fails a request whose stream stays silent for timeoutMs, including during SSE consumption;
      // long outputs that keep streaming are allowed to finish.
      stream.setTimeout(timeoutMs, () => stream.destroy(new Error('Request timed out.')));
      stream.once('close', () => reject(new Error('Request closed.')));
      stream.on('error', reject);
      stream.once('response', responseHeaders => {
        metrics.headers_at = Date.now();
        metrics.status = responseHeaders[':status'];
        metrics.rate_headers = rateHeaders(Object.entries(responseHeaders), secrets);
        if ([301, 302, 303, 307, 308].includes(responseHeaders[':status']) && responseHeaders.location !== undefined) {
          stream.close(constants.NGHTTP2_CANCEL);
          reject(new Error('Redirect refused.'));
          return;
        }
        if (url === endpoint && metrics.status === 429) {
          const value = responseHeaders['retry-after'];
          const seconds = typeof value === 'string' && /^\d+(?:\.\d+)?$/.test(value) ? Number(value) : NaN;
          const waitMs = Number.isFinite(seconds * 1000) ? seconds * 1000 : rateLimitDelayMs;
          pausedUntil = Math.max(pausedUntil, performance.now() + waitMs);
          const pause_id = ++pauseNumber, started_at = Date.now();
          pauses.set(pause_id, started_at);
          event({ type: 'pause_start', run_id: runId, pause_id, started_at, at: started_at,
            retry_after: metrics.rate_headers['retry-after'] ?? null });
        }
        resolve({ status: metrics.status, ok: metrics.status >= 200 && metrics.status < 300, body: stream });
      });
      stream.end(payload);
    });
  }

  async function responseText(response) {
    const chunks = [];
    for await (const chunk of response.body) chunks.push(chunk);
    return Buffer.concat(chunks).toString('utf8');
  }

  async function lock() {
    const lockPath = `${authPath}.refresh.lock`;
    const deadline = Date.now() + timeoutMs + 30_000;
    const sameFile = (a, b) => a.dev === b.dev && a.ino === b.ino;
    async function owns(identity) {
      try { return sameFile(identity, await stat(lockPath)); }
      catch (error) { if (error.code === 'ENOENT') return false; throw error; }
    }
    async function remove(identity) {
      if (await owns(identity)) {
        try { await unlink(lockPath); }
        catch (error) { if (error.code !== 'ENOENT') throw error; }
      }
    }
    async function reclaim() {
      let handle;
      try {
        handle = await open(lockPath, 'r');
        const identity = await handle.stat();
        let owner;
        try { owner = JSON.parse(await handle.readFile('utf8')); }
        catch (error) { if (!(error instanceof SyntaxError)) throw error; }
        let dead = false;
        if (Number.isSafeInteger(owner?.pid) && owner.pid > 0) {
          try { process.kill(owner.pid, 0); }
          catch (error) {
            if (error.code === 'ESRCH') dead = true;
            else if (error.code !== 'EPERM') throw error;
          }
        }
        // mtime also recovers a crash between exclusive creation and metadata writing.
        const createdAt = Number.isFinite(owner?.createdAt) ? owner.createdAt : identity.mtimeMs;
        if (dead || Date.now() - createdAt > 120_000) await remove(identity);
      } catch (error) { if (error.code !== 'ENOENT') throw authError(); }
      finally { await handle?.close(); }
    }
    for (;;) {
      try {
        const handle = await open(lockPath, 'wx', 0o600);
        const identity = await handle.stat();
        try { await handle.writeFile(JSON.stringify({ pid: process.pid, createdAt: Date.now() })); }
        catch { await handle.close(); await unlink(lockPath); throw authError(); }
        return {
          async check() { if (!await owns(identity)) throw authError(); },
          async release() { await handle.close(); await remove(identity); },
        };
      } catch (error) {
        if (error.code !== 'EEXIST') throw authError();
        await reclaim();
        if (Date.now() >= deadline) throw authError();
        await delay(25);
      }
    }
  }

  async function refresh(auth, reason, context) {
    const pending = refreshes.get(authPath);
    if (pending) { await pending; return readAuth(); }
    const promise = (async () => {
      const lease = await lock();
      try {
        let latest = await readAuth();
        if (latest.tokens.access_token !== auth.tokens.access_token ||
          (reason !== 'unauthorized_recovery' && !refreshReason(latest))) return latest;
        if (typeof latest.tokens.refresh_token !== 'string' || !latest.tokens.refresh_token) throw authError();
        const data = await logged('refresh', async metrics => {
          const response = await send(refreshUrl, { grant_type: 'refresh_token', client_id: CLIENT_ID,
            refresh_token: latest.tokens.refresh_token, refresh_reason: reason }, metrics);
          if (!response.ok) { response.body.close(constants.NGHTTP2_CANCEL); throw authError(); }
          return JSON.parse(await responseText(response));
        }, { ...context, queued_at: Date.now(), attempt: 1 });
        for (const key of ['access_token', 'refresh_token', 'id_token']) {
          if (data[key] !== undefined && (typeof data[key] !== 'string' || !data[key])) throw authError();
          if (typeof data[key] === 'string') secrets.add(data[key]);
        }
        if (!data.access_token) throw authError();
        // Preserve fields written by other consumers while the refresh was in flight.
        await lease.check();
        const current = await readAuth();
        if (current.tokens.access_token !== latest.tokens.access_token ||
          current.tokens.refresh_token !== latest.tokens.refresh_token) return current;
        latest = current;
        for (const key of ['access_token', 'refresh_token', 'id_token'])
          if (data[key] !== undefined) latest.tokens[key] = data[key];
        latest.last_refresh = new Date().toISOString();
        const temporary = `${authPath}.${randomUUID()}.tmp`;
        let handle;
        try {
          handle = await open(temporary, 'wx', 0o600);
          await handle.writeFile(JSON.stringify(latest, null, 2) + '\n');
          await handle.sync(); await handle.close(); handle = undefined;
          await lease.check();
          await rename(temporary, authPath);
        } finally {
          await handle?.close();
          await unlink(temporary).catch(error => { if (error.code !== 'ENOENT') throw error; });
        }
        return latest;
      } finally { await lease.release(); }
    })().catch(() => { throw authError(); });
    refreshes.set(authPath, promise);
    try { return await promise; }
    finally { if (refreshes.get(authPath) === promise) refreshes.delete(authPath); }
  }

  async function run({ model, effort, instructions, input, schema, validate, telemetry = {} }) {
    const body = { model, reasoning: { effort }, instructions, input, tools: [], tool_choice: 'auto',
      parallel_tool_calls: false, store: false, stream: true, include: [],
      ...(schema === undefined ? {} : { text: { format: { type: 'json_schema', name: 'codex_output_schema', strict: true, schema } } }) };
    let recovered = false, httpAttempt = 0, previousFailure = null;
    let queuedAt = telemetry.queued_at ?? Date.now();
    const context = () => ({ file: telemetry.file, batch_id: telemetry.batch_id, batch_ids: telemetry.batch_ids,
      estimated_input_tokens: telemetry.estimated_input_tokens, name_count: telemetry.name_count, phase: telemetry.phase,
      queued_at: queuedAt, attempt: ++httpAttempt, previous_failure: previousFailure });
    for (let attempt = 0; ; attempt++) {
      if (terminal) throw terminal;
      try {
        let auth = await readAuth();
        const reason = refreshReason(auth);
        if (reason) auth = await refresh(auth, reason, telemetry);
        for (;;) {
          await waitForRateLimit();
          if (terminal) throw terminal;
          const result = await logged('model', async metrics => {
            let response;
            try {
              response = await send(endpoint, body, metrics, { Authorization: `Bearer ${auth.tokens.access_token}`,
                'ChatGPT-Account-ID': auth.tokens.account_id, originator: 'codex_cli_rs',
                Accept: 'text/event-stream', 'User-Agent': 'codex-responses-client' });
              if (response.status === 401) { response.body.close(constants.NGHTTP2_CANCEL); return { unauthorized: true }; }
              if (!response.ok) {
                let error;
                try { error = JSON.parse(await responseText(response)).error; } catch { /* Only structured codes classify quota errors. */ }
                if (hasQuotaError(error)) throw quotaError();
                if (response.status === 429) return { rateLimited: true };
                if (response.status >= 500) throw retryError(`Responses HTTP ${response.status}.`);
                throw new Error(`Responses HTTP ${response.status}.`);
              }
              const text = await consume(response, metrics);
              let output = text;
              if (schema !== undefined) {
                try { output = JSON.parse(text); } catch { throw retryError('Responses output is not JSON.'); }
              }
              if (validate) {
                try { await validate(output); } catch { throw retryError('Responses output failed validation.'); }
              }
              return { output, usage: metrics.usage };
            } catch (error) {
              if (error.code === 'USAGE_LIMIT') throw stop(error);
              if (error.retryable || /^Responses HTTP \d+\.$/.test(error.message)) throw error;
              throw retryError('Responses network or stream failure.');
            } finally { if (response?.body) response.body.close(constants.NGHTTP2_CANCEL); }
          }, context());
          queuedAt = Date.now();
          if (result.rateLimited) { previousFailure = 'HTTP_429'; continue; }
          if (!result.unauthorized) return result;
          if (recovered) throw authError();
          recovered = true;
          previousFailure = 'HTTP_401';
          auth = await refresh(auth, 'unauthorized_recovery', telemetry);
        }
      } catch (error) {
        if (['AUTH', 'USAGE_LIMIT'].includes(error.code)) throw stop(error);
        if (!error.retryable || attempt === 3) throw new Error(error.message);
        previousFailure = error.code ?? error.message;
        queuedAt = Date.now();
        await delay(retryDelayMs * 2 ** attempt);
      }
    }
  }

  function pump() {
    while (active < concurrency && queue.length) {
      const job = queue.shift(); active++;
      (terminal ? Promise.reject(terminal) : run(job.request)).then(job.resolve, job.reject).finally(() => {
        active--; pump();
        if (!active && !queue.length) drain.splice(0).forEach(resolve => resolve());
      });
    }
  }
  return {
    respond(request) {
      if (terminal) return Promise.reject(terminal);
      if (!accepting) return Promise.reject(new Error('Responses client is closed.'));
      return new Promise((resolve, reject) => { queue.push({ request: { ...request, telemetry: { ...request.telemetry, queued_at: request.telemetry?.queued_at ?? Date.now() } }, resolve, reject }); pump(); });
    },
    async close() {
      accepting = false;
      if (active || queue.length) await new Promise(resolve => drain.push(resolve));
      endPauses();
      await Promise.all([...connections].filter(session => !session.destroyed).map(session => new Promise(resolve => {
        session.once('close', resolve);
        session.close();
      })));
    },
  };
}
