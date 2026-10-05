// ABOUTME: HTTP for adapters' API checks: a Chrome-shaped TLS client (impit) leaving from a random
// ABOUTME: slot of the ISP proxy pool, the same pool the apps' production clients call from.
import { ispProxyAt } from "./config.mjs";

// The proxy URL of one ISP pool slot, picked at random from 1..ISP_PROXY_COUNT.
export function ispSlotUrl(env, random = Math.random) {
  if (!env.ISP_PROXY_URL) throw new Error("No ISP_PROXY_URL in ~/.config/secrets-manager/.env; never call an app from the home IP.");
  const count = Math.max(1, Number(env.ISP_PROXY_COUNT) || 1);
  return ispProxyAt(env.ISP_PROXY_URL, 1 + Math.floor(random() * count));
}

// fetch(url, init) through a random ISP slot with a Chrome TLS fingerprint, which the apps'
// Cloudflare and JA3 walls require. Returns a fetch Response.
export async function ispFetch(url, init = {}) {
  const { Impit } = await import("impit");
  const client = new Impit({ browser: "chrome", proxyUrl: ispSlotUrl(process.env) });
  return client.fetch(url, init);
}
