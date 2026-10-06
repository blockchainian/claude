// ABOUTME: Rents a temporary phone number from HeroSMS and reads the verification SMS it receives.
// ABOUTME: HeroSMS speaks the SMS-Activate protocol; the pure parsers are tested, the calls are not.

import * as config from "./config.mjs";

const API_URL = "https://hero-sms.com/stubs/handler_api.php";

// The SMS-Activate service code shared by Google, Gmail and YouTube.
export const GOOGLE_SERVICE = "go";

// setStatus codes — what to tell HeroSMS about an activation.
export const STATUS_SENT = 1; // the number was entered; ready to receive
export const STATUS_RESEND = 3; // ask for another SMS
export const STATUS_COMPLETE = 6; // code received and used — close it out
export const STATUS_CANCEL = 8; // give the number up and refund

// --- pure parsers (tested) ---------------------------------------------------

// The balance from a getBalance reply (`ACCESS_BALANCE:12.34`), or null on an error token.
export function parseBalance(text) {
  const m = /ACCESS_BALANCE:([0-9.]+)/.exec(text || "");
  return m ? parseFloat(m[1]) : null;
}

// The activation id and phone from a getNumber reply (`ACCESS_NUMBER:id:phone`), or null when no
// number was granted (`NO_NUMBERS`, `NO_BALANCE`, an empty body).
export function parseNumber(text) {
  const m = /^ACCESS_NUMBER:([^:]+):(.+)$/.exec((text || "").trim());
  return m ? { activationId: m[1], phone: m[2] } : null;
}

// Interpret a getStatus reply. state is "ok" (a code is present), "waiting" (any WAIT_* variant,
// with the last seen code on a retry), "cancelled", or "unknown" (the raw reply is kept).
export function parseStatus(text) {
  text = (text || "").trim();
  if (text.startsWith("STATUS_OK:")) return { state: "ok", code: text.slice("STATUS_OK:".length) };
  if (text.startsWith("STATUS_WAIT_RETRY:")) return { state: "waiting", lastCode: text.slice("STATUS_WAIT_RETRY:".length) };
  if (text === "STATUS_WAIT_CODE" || text === "STATUS_WAIT_RESEND") return { state: "waiting" };
  if (text === "STATUS_CANCEL") return { state: "cancelled" };
  return { state: "unknown", raw: text };
}

// Exponential-backoff delays (ms) for retrying a flapping HeroSMS request: 1s, 2s, 4s, … doubling,
// each admitted while the time already spent is under the budget and the delay itself fits it — so a
// budget below the base yields no retries. Pure, so it's tested.
export function backoffDelays(maxTotalMs = 60000, baseMs = 1000) {
  const delays = [];
  let delay = baseMs;
  let total = 0;
  while (total < maxTotalMs && delay <= maxTotalMs) {
    delays.push(delay);
    total += delay;
    delay *= 2;
  }
  return delays;
}

// The countries in a getPrices object (`{country: {service: {cost, count}}}`) that offer `service`
// in stock at cost ≤ maxPrice, cheapest first. `country` stays the numeric id as a string. Countries
// whose id is in `blacklist` (numbers or strings) are dropped — a country that accepts a number but
// never delivers Google's code only wastes a rent, so it must never be picked whatever the price.
export function affordableCountries(prices, service, maxPrice, blacklist = []) {
  const denied = new Set(blacklist.map(String));
  const out = [];
  for (const [country, services] of Object.entries(prices || {})) {
    if (denied.has(String(country))) continue;
    const row = services && services[service];
    if (!row) continue;
    const cost = Number(row.cost);
    const count = Number(row.count ?? 0);
    if (Number.isFinite(cost) && cost <= maxPrice && count > 0) out.push({ country, cost, count });
  }
  return out.sort((a, b) => a.cost - b.cost);
}

// --- HeroSMS calls (not unit-tested; they spend money) -----------------------

function requireKey() {
  const key = config.heroSmsKey();
  if (!key) throw new Error("No HeroSMS key. Add HERO_SMS_API_KEY to ~/.config/secrets-manager/.env.");
  return key;
}

// One GET against the handler, returning the trimmed body. Bare error tokens that mean the request
// itself is malformed (bad key/action/service) throw; per-action outcomes (NO_NUMBERS, the STATUS_*
// replies) are returned for the caller to read.
async function call(action, params = {}) {
  const url = new URL(API_URL);
  url.searchParams.set("api_key", requireKey());
  url.searchParams.set("action", action);
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== null && v !== "") url.searchParams.set(k, String(v));
  }
  // A transient failure (network "fetch failed", DNS, a reset) must NOT crash the login or burn a
  // fresh number — retry the SAME request with exponential backoff, logging which error it was so it
  // can be diagnosed. A protocol error (bad key/action/service) is permanent and surfaces at once.
  const delays = backoffDelays();
  for (let attempt = 0; ; attempt++) {
    try {
      const res = await fetch(url, { headers: { "User-Agent": "Mozilla/5.0" } });
      const text = (await res.text()).trim();
      if (/^(BAD_KEY|NO_KEY|BAD_ACTION|BAD_SERVICE|ERROR_SQL|BANNED)/.test(text)) throw new Error(`HeroSMS: ${text}`);
      return text;
    } catch (e) {
      if (String(e.message).startsWith("HeroSMS: ")) throw e; // permanent protocol error — don't retry
      console.log(`  sms: "${action}" request failed (attempt ${attempt + 1}): ${e.message}`);
      const wait = delays[attempt];
      if (wait === undefined) throw new Error(`HeroSMS "${action}" failed after ${delays.length + 1} attempts: ${e.message}`);
      await new Promise((r) => setTimeout(r, wait));
    }
  }
}

export async function getBalance() {
  const balance = parseBalance(await call("getBalance"));
  if (balance === null) throw new Error("HeroSMS: could not read balance");
  return balance;
}

// The raw getPrices object for a service (optionally one country), `{country: {service: {cost, count}}}`.
export async function getPrices({ service = GOOGLE_SERVICE, country } = {}) {
  const text = await call("getPrices", { service, country });
  try {
    return JSON.parse(text);
  } catch {
    throw new Error(`HeroSMS getPrices returned non-JSON: ${text}`);
  }
}

// Rent a number for `service` (optionally pinned to a `country` id, capped at `maxPrice`). Returns
// {activationId, phone}. Throws a clear message when the pool is empty or the balance is short.
export async function requestNumber({ service = GOOGLE_SERVICE, country, maxPrice } = {}) {
  const text = await call("getNumber", { service, country, maxPrice });
  if (text.startsWith("NO_NUMBERS")) throw new Error("HeroSMS: no numbers for that service/country/price");
  if (text.startsWith("NO_BALANCE")) throw new Error("HeroSMS: insufficient balance");
  const parsed = parseNumber(text);
  if (!parsed) throw new Error(`HeroSMS getNumber returned: ${text}`);
  return parsed;
}

// Tell HeroSMS what happened to an activation (STATUS_SENT/RESEND/COMPLETE/CANCEL).
export async function setStatus(activationId, status) {
  return call("setStatus", { id: activationId, status });
}

// Poll getStatus until a code arrives or the deadline passes. Returns {state:"ok", code} on
// delivery, {state:"cancelled"} if HeroSMS dropped it, or {state:"timeout", activationId} so the
// caller decides between cancel (refund) and resend. Never throws on an ordinary wait.
export async function pollCode(activationId, { timeoutS = 300, intervalS = 5 } = {}) {
  const deadline = Date.now() + timeoutS * 1000;
  while (Date.now() < deadline) {
    let status;
    try {
      status = parseStatus(await call("getStatus", { id: activationId }));
    } catch (e) {
      // The backoff inside call() already exhausted its retry budget, so HeroSMS is genuinely
      // unreachable — give this number up cleanly (the caller cancels/refunds) instead of crashing
      // the whole login.
      console.log(`  sms: getStatus unreachable after retries; giving this number up: ${e.message}`);
      return { state: "timeout", activationId };
    }
    if (status.state === "ok") return { state: "ok", code: status.code };
    if (status.state === "cancelled") return { state: "cancelled", activationId };
    await new Promise((resolve) => setTimeout(resolve, intervalS * 1000));
  }
  return { state: "timeout", activationId };
}
