import { requireEnv, loadEnvFile } from "../../fetch-x-mentions/scripts/env.mjs";
// ABOUTME: Fetches all App Store written reviews for an Apple app across storefronts,
// ABOUTME: rotating a fresh residential-proxy exit IP per request, resumable per storefront.
import { realpathSync } from "node:fs";
//
// Usage:
//   node ${CLAUDE_PLUGIN_ROOT}/skills/fetch-app-reviews/scripts/fetch-app-reviews.mjs <appleId> [appName]
//
// - <appleId>  numeric App Store id, e.g. 6741115427
// - [appName]  output slug; if omitted it is derived from the app's store name
// - Reads RESIDENTIAL_PROXY_URL from the env file. A rotating proxy endpoint hands out a
//   new exit IP per CONNECTION, not per request, so a reused undici tunnel pins one IP —
//   getJson therefore builds and closes a fresh ProxyAgent for every request, which is what
//   actually rotates the IP and defeats Apple's per-IP throttling.
// - Writes docs/intel/reviews/<appName>.json as { appId, appName, updatedAt, complete,
//   countriesDone, count, reviews }, checkpointing after each storefront. `complete` is
//   true only when every storefront reached a confirmed real end.
// - Per-storefront completion (countriesDone) carries across runs, so a retry only
//   re-fetches storefronts that have not yet confirmed an end. It always pages a storefront
//   to a confirmed end (no delta short-circuit); correctness over re-run speed.

import { readFile, writeFile, mkdir } from "node:fs/promises";
import { pathToFileURL, fileURLToPath } from "node:url";
import { join } from "node:path";
const isMain = process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) loadEnvFile(new URL(".env", import.meta.url));

let ProxyAgent;
try { ({ ProxyAgent } = await import("undici")); }
catch (e) {
  if (e.code !== 'ERR_MODULE_NOT_FOUND') throw e;
  throw new Error('Install dependencies: npm install --prefix "${CLAUDE_PLUGIN_ROOT}/skills/fetch-app-reviews/scripts"', {cause:e});
}

const API = "https://apps.apple.com/api/apps/v1/catalog";
const UA =
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 " +
  "(KHTML, like Gecko) Version/17.0 Safari/605.1.15";
const LIMIT = 20;
const MAX_RETRIES = 20; // 429/5xx get many fast fresh-IP retries before giving up
const COUNTRY_CONCURRENCY = 4; // storefronts fetched in parallel per app
const PAGE_WINDOW = 4; // offsets fetched in parallel within one storefront

// Storefronts worth querying. The long tail of ~175 storefronts holds ~0 reviews for
// these apps; this set covers effectively all of them.
const COUNTRIES = [
  "us",
  "gb",
  "ca",
  "au",
  "ie",
  "nz",
  "de",
  "fr",
  "it",
  "es",
  "nl",
  "be",
  "at",
  "ch",
  "pt",
  "se",
  "no",
  "dk",
  "fi",
  "pl",
  "cz",
  "sk",
  "hu",
  "ro",
  "gr",
  "tr",
  "ru",
  "ua",
  "jp",
  "kr",
  "cn",
  "hk",
  "tw",
  "sg",
  "my",
  "th",
  "id",
  "ph",
  "vn",
  "in",
  "pk",
  "bd",
  "lk",
  "ae",
  "sa",
  "qa",
  "kw",
  "il",
  "eg",
  "ng",
  "za",
  "ke",
  "ma",
  "br",
  "mx",
  "ar",
  "cl",
  "co",
  "pe",
];

const REVIEWS_DIR = join("docs", "intel", "reviews"); // run from the repo root
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function proxyConfig() {
  const url = process.env.RESIDENTIAL_PROXY_URL || process.env.HTTPS_PROXY;
  if (!url) {
    throw new Error(
      "RESIDENTIAL_PROXY_URL (or HTTPS_PROXY) is required in the calling script's .env.",
    );
  }
  const u = new URL(url);
  const token =
    u.username &&
    `Basic ${Buffer.from(`${decodeURIComponent(u.username)}:${decodeURIComponent(u.password)}`).toString("base64")}`;
  return { uri: `${u.protocol}//${u.host}`, token };
}

// A fresh agent per request => a fresh CONNECT tunnel => a fresh residential exit IP.
function newAgent() {
  const proxy = proxyConfig();
  return new ProxyAgent(
    proxy.token ? { uri: proxy.uri, token: proxy.token } : proxy.uri,
  );
}

async function getJson(url) {
  for (let attempt = 0; attempt < MAX_RETRIES; attempt++) {
    const agent = newAgent();
    let res;
    try {
      res = await fetch(url, {
        dispatcher: agent,
        headers: {
          "User-Agent": UA,
          Accept: "application/json",
          Referer: "https://apps.apple.com/",
        },
      });
    } catch {
      await agent.close().catch(() => {});
      await sleep(200); // network error: retry quickly on a fresh IP
      continue;
    }
    if (res.status === 200) {
      const body = await res.json().catch(() => null);
      await agent.close().catch(() => {});
      if (body) return { status: 200, body };
      await sleep(200);
      continue; // malformed body: retry on a fresh IP rather than trust it
    }
    await res.body?.cancel?.();
    await agent.close().catch(() => {});
    if (res.status === 404) return { status: 404, body: null };
    // 429 (a dirty exit IP) or 5xx: a fresh IP is one retry away — retry fast.
    if (res.status === 429 || res.status >= 500) {
      await sleep(Math.min(1500, 100 * (attempt + 1)));
      continue;
    }
    // A genuine 4xx (e.g. 400): a few retries, then give up on this URL.
    if (attempt >= 4) return { status: res.status, body: null };
    await sleep(300);
  }
  return { status: 0, body: null };
}

function reviewUrl(appId, cc, offset) {
  return `${API}/${cc}/apps/${appId}/reviews?l=en-US&platform=web&limit=${LIMIT}&offset=${offset}`;
}

function normalize(item, appName, appId, cc) {
  const a = item.attributes ?? {};
  const dr = a.developerResponse ?? null;
  return {
    id: item.id,
    app: appName,
    app_id: appId,
    country: cc,
    rating: a.rating ?? null,
    title: a.title ?? null,
    body: a.review ?? null,
    userName: a.userName ?? null,
    date: a.date ?? null,
    isEdited: a.isEdited ?? null,
    developerResponseBody: dr?.body ?? null,
    developerResponseModified: dr?.modified ?? null,
  };
}

// Runs `worker` over `items` with at most `concurrency` in flight.
async function poolMap(items, concurrency, worker) {
  const queue = [...items];
  const runners = Array.from(
    { length: Math.min(concurrency, queue.length) },
    async () => {
      while (queue.length) await worker(queue.shift());
    },
  );
  await Promise.all(runners);
}

async function fetchOffset(appId, cc, offset) {
  const { status, body } = await getJson(reviewUrl(appId, cc, offset));
  return { status, data: status === 200 && body ? (body.data ?? []) : null };
}

// Fetches one storefront into the shared byId map, paging offsets in parallel windows.
// Reviews are contiguous from offset 0 to the storefront's end. Returns ok:false on any
// signal that the "end" might be a proxy artifact rather than the real end, so the app is
// left incomplete and refetched — a truncated storefront must never look complete.
async function fetchCountry(appId, appName, cc, byId) {
  const addPage = (data) => {
    for (const item of data) {
      if (!item.id || byId.has(item.id)) continue;
      byId.set(item.id, normalize(item, appName, appId, cc));
    }
  };
  const hasData = (p) => p.data != null && p.data.length > 0;

  let base = 0;
  for (;;) {
    const offsets = Array.from(
      { length: PAGE_WINDOW },
      (_, i) => base + i * LIMIT,
    );
    const pages = await Promise.all(
      offsets.map((o) => fetchOffset(appId, cc, o)),
    );

    // A deterministic client error means this storefront does not serve the reviews
    // endpoint (e.g. Bangladesh returns 400 for every offset). Treat it as an available
    // "0 reviews" result, recorded as unavailable, so the app can still complete.
    if (
      pages.some(
        (p) => p.status === 400 || p.status === 403 || p.status === 451,
      )
    ) {
      return { ok: true, unavailable: true };
    }
    // Any other non-terminal status is a transient failure: do not trust this window.
    if (pages.some((p) => p.status !== 200 && p.status !== 404))
      return { ok: false };

    const emptyIdx = pages.findIndex((p) => !hasData(p));
    if (emptyIdx === -1) {
      pages.forEach((p) => addPage(p.data));
      base += PAGE_WINDOW * LIMIT;
      continue;
    }
    // A page after the first empty one still has data => the empty page was a hole.
    if (pages.slice(emptyIdx + 1).some(hasData)) return { ok: false };

    for (let i = 0; i < emptyIdx; i++) addPage(pages[i].data);
    // Confirm the boundary is a real end by re-fetching that offset on a fresh IP.
    const confirm = await fetchOffset(appId, cc, offsets[emptyIdx]);
    if (confirm.status !== 200 && confirm.status !== 404) return { ok: false };
    if (hasData(confirm)) return { ok: false };
    return { ok: true };
  }
}

// Fetches every reachable written review for one app and writes it to
// docs/intel/reviews/<appName>.json, checkpointing after each storefront.
export async function fetchAppReviews(appId, appNameOverride, opts = {}) {
  const appName = await resolveAppName(appId, appNameOverride);

  await mkdir(REVIEWS_DIR, { recursive: true });
  const outPath = join(REVIEWS_DIR, `${appName}.json`);

  const existing = await loadExisting(outPath);
  const byId = new Map((existing.reviews ?? []).map((r) => [r.id, r]));
  const doneCountries = new Set(existing.countriesDone ?? []);
  const unavailableCountries = new Set(existing.countriesUnavailable ?? []);
  const startCount = byId.size;

  const isComplete = () => COUNTRIES.every((cc) => doneCountries.has(cc));
  const save = () =>
    writeFile(
      outPath,
      JSON.stringify(
        {
          appId,
          appName,
          updatedAt: new Date().toISOString(),
          complete: isComplete(),
          countriesDone: [...doneCountries],
          countriesUnavailable: [...unavailableCountries],
          count: byId.size,
          reviews: [...byId.values()],
        },
        null,
        1,
      ),
    );

  let saveChain = Promise.resolve();
  const scheduleSave = () => {
    saveChain = saveChain.then(save).catch(() => {});
  };

  const pending = COUNTRIES.filter((cc) => !doneCountries.has(cc));
  await poolMap(pending, COUNTRY_CONCURRENCY, async (cc) => {
    const { ok, unavailable } = await fetchCountry(appId, appName, cc, byId);
    if (ok) {
      doneCountries.add(cc);
      if (unavailable) unavailableCountries.add(cc);
    }
    scheduleSave();
    opts.onCountry?.({ appName, cc, total: byId.size, ok });
  });
  await saveChain;
  await save();

  return {
    appName,
    appId,
    count: byId.size,
    added: byId.size - startCount,
    complete: isComplete(),
    outPath,
  };
}

async function resolveAppName(appId, override) {
  if (override) return override;
  const { body } = await getJson(
    `https://itunes.apple.com/lookup?id=${appId}&country=us`,
  );
  const name = body?.results?.[0]?.trackName;
  if (!name) {
    throw new Error(
      `Could not resolve a name for app ${appId}; pass one explicitly.`,
    );
  }
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

async function loadExisting(outPath) {
  try {
    const parsed = JSON.parse(await readFile(outPath, "utf8"));
    return Array.isArray(parsed) ? { reviews: parsed } : parsed;
  } catch {
    return { reviews: [] };
  }
}

async function main() {
  const [appId, appNameArg] = process.argv.slice(2);
  if (!appId || !/^\d+$/.test(appId)) {
    console.error(
      "Usage: node fetch-app-reviews.mjs <appleId> [appName]",
    );
    process.exit(1);
  }
  const res = await fetchAppReviews(appId, appNameArg, {
    onCountry: ({ cc, total, ok }) =>
      process.stderr.write(`  ${cc}: total=${total}${ok ? "" : " FAILED"}\n`),
  });
  console.log(
    `${res.appName}: ${res.count} reviews${res.complete ? "" : " (INCOMPLETE)"} -> ${res.outPath}`,
  );
}

if (isMain) {
  await main();
}
