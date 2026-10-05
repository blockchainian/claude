// ABOUTME: Proxy plumbing shared by the browser and API paths: an undici dispatcher for a proxy URL,
// ABOUTME: and the exit check that refuses to open a browser through a residential exit that is down.
import { ProxyAgent, fetch } from "undici";

// A tiny Google endpoint the sign-in pages load anyway; any answer proves the tunnel works.
const PROBE_URL = "https://www.gstatic.com/generate_204";
const PROBE_TIMEOUT_MS = 15000;
const PROBE_ATTEMPTS = 2;

// The proxy exit cannot reach the internet (e.g. CONNECT answered 522): opening a browser through it
// only times out on half-loaded pages, so the run reports this instead.
export class ProxyExitDown extends Error {}

// An undici dispatcher that sends requests through `proxyUrl`, carrying its credentials.
export function dispatcherFor(proxyUrl) {
  const u = new URL(proxyUrl);
  const uri = `${u.protocol}//${u.host}`;
  if (!u.username) return new ProxyAgent(uri);
  const token = `Basic ${Buffer.from(`${decodeURIComponent(u.username)}:${decodeURIComponent(u.password)}`).toString("base64")}`;
  return new ProxyAgent({ uri, token });
}

// Resolve when the exit behind `proxyUrl` tunnels to the probe target (any HTTP status counts);
// throw ProxyExitDown after every attempt failed. The message names the proxy host, never the
// credentials in its username or password.
export async function assertExitUp(proxyUrl, { fetchImpl = fetch } = {}) {
  let last;
  for (let attempt = 0; attempt < PROBE_ATTEMPTS; attempt++) {
    try {
      const res = await fetchImpl(PROBE_URL, { dispatcher: dispatcherFor(proxyUrl), signal: AbortSignal.timeout(PROBE_TIMEOUT_MS) });
      await res.body?.cancel?.();
      return;
    } catch (e) {
      last = e;
    }
  }
  const reason = [last?.message, last?.cause?.message].filter(Boolean).join(": ");
  throw new ProxyExitDown(`proxy exit down: ${new URL(proxyUrl).host} could not reach ${new URL(PROBE_URL).host} (${reason})`);
}
