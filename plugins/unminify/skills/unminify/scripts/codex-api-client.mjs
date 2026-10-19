// Reusable direct Codex Responses client backed by a ChatGPT file login.
import { readFile, open, rename, unlink, stat } from 'node:fs/promises';
import { homedir } from 'node:os';
import { resolve, join } from 'node:path';
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
 * log receives { kind, status, duration_ms, usage, rate_headers } for each HTTP attempt.
 * close() drains accepted calls and prevents further calls.
 */
export function createCodexClient({ concurrency = 20, codexHome = process.env.CODEX_HOME || join(homedir(), '.codex'),
  log = () => {}, baseUrl = 'https://chatgpt.com/backend-api/codex',
  refreshUrl = 'https://auth.openai.com/oauth/token', timeoutMs = 120_000, retryDelayMs = 500 } = {}) {
  if (!Number.isSafeInteger(concurrency) || concurrency < 1) throw new Error('Invalid concurrency.');
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0 || !Number.isFinite(retryDelayMs) || retryDelayMs < 0)
    throw new Error('Invalid request timing.');
  const authPath = resolve(codexHome, 'auth.json');
  const endpoint = `${baseUrl.replace(/\/$/, '')}/responses`;
  const secrets = new Set();
  let terminal, accepting = true, active = 0;
  const queue = [], drain = [];

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

  async function logged(kind, operation) {
    const started = performance.now();
    const metrics = { kind, status: null, duration_ms: 0, usage: null, rate_headers: {} };
    try { return await operation(metrics); }
    finally {
      metrics.duration_ms = performance.now() - started;
      try { await log(metrics); } catch { throw new Error('Request logging failed.'); }
    }
  }
  async function send(url, body, metrics, headers = {}) {
    const response = await fetch(url, { method: 'POST', redirect: 'error',
      signal: AbortSignal.timeout(timeoutMs), headers: { 'Content-Type': 'application/json', ...headers },
      body: JSON.stringify(body) });
    metrics.status = response.status;
    metrics.rate_headers = rateHeaders(response.headers, secrets);
    return response;
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

  async function refresh(auth, reason) {
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
          if (!response.ok) { await response.body?.cancel(); throw authError(); }
          return response.json();
        });
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

  async function run({ model, effort, instructions, input, schema, validate }) {
    const body = { model, reasoning: { effort }, instructions, input, tools: [], tool_choice: 'auto',
      parallel_tool_calls: false, store: false, stream: true, include: [],
      ...(schema === undefined ? {} : { text: { format: { type: 'json_schema', name: 'codex_output_schema', strict: true, schema } } }) };
    let recovered = false;
    for (let attempt = 0; ; attempt++) {
      if (terminal) throw terminal;
      try {
        let auth = await readAuth();
        const reason = refreshReason(auth);
        if (reason) auth = await refresh(auth, reason);
        for (;;) {
          if (terminal) throw terminal;
          const result = await logged('model', async metrics => {
            let response;
            try {
              response = await send(endpoint, body, metrics, { Authorization: `Bearer ${auth.tokens.access_token}`,
                'ChatGPT-Account-ID': auth.tokens.account_id, originator: 'codex_cli_rs',
                Accept: 'text/event-stream', 'User-Agent': 'codex-responses-client' });
              if (response.status === 401) { await response.body?.cancel(); return { unauthorized: true }; }
              if (!response.ok) {
                let error;
                try { error = JSON.parse(await response.text()).error; } catch { /* Only structured codes classify quota errors. */ }
                if (hasQuotaError(error)) throw quotaError();
                if (response.status >= 500 || response.status === 429) throw retryError(`Responses HTTP ${response.status}.`);
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
              if (error.code === 'USAGE_LIMIT') { terminal = error; throw error; }
              if (error.retryable || /^Responses HTTP \d+\.$/.test(error.message)) throw error;
              throw retryError('Responses network or stream failure.');
            } finally { if (response?.body && !response.body.locked) await response.body.cancel().catch(() => {}); }
          });
          if (!result.unauthorized) return result;
          if (recovered) throw authError();
          recovered = true;
          auth = await refresh(auth, 'unauthorized_recovery');
        }
      } catch (error) {
        if (['AUTH', 'USAGE_LIMIT'].includes(error.code)) { terminal = error; throw error; }
        if (!error.retryable || attempt === 3) throw new Error(error.message);
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
      return new Promise((resolve, reject) => { queue.push({ request, resolve, reject }); pump(); });
    },
    close() {
      accepting = false;
      return active || queue.length ? new Promise(resolve => drain.push(resolve)) : Promise.resolve();
    },
  };
}
