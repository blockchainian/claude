import { requireEnv, loadEnvFile } from "./env.mjs";
// ABOUTME: Fetches X/Twitter mentions of an app over a date range, one authenticated GraphQL
// ABOUTME: SearchTimeline per day, sharded across accounts through the residential proxy, resumable.
//
// Usage (config from the calling script's .env loaded automatically):
//   node ${CLAUDE_PLUGIN_ROOT}/skills/fetch-x-mentions/scripts/fetch-x-mentions.mjs \
//     <slug> "<query>" [sinceYYYY-MM-DD] [untilYYYY-MM-DD] [--daily-limit <n>] [--refill [<n>]]
//   <slug> names the output dir docs/intel/x/<slug>/ holding tweets.jsonl and tweets.out.json.
//   --daily-limit caps how many tweets one day may yield (default 1000, rounded up to whole
//   pages of 20). Days that hit the cap are truncated, newest tweets first.
//   --refill [n] also re-fetches the gap days: those recorded with 0 tweets, or cut at a page
//   boundary below the limit (X answers a throttled account with an empty page instead of a
//   429, which ends the day early). A gap day is fetched from its oldest held tweet backwards,
//   so only the missing part is requested, and its `refill` count goes up by one. Only gaps
//   refilled at most n times are taken (n defaults to 0: never refilled), so a day that stays
//   a gap after a refill is left alone until asked for again with a higher n.
//
// Example (alpha — @alpha account created 2024-12-13, so that is the natural start):
//   node ${CLAUDE_PLUGIN_ROOT}/skills/fetch-x-mentions/scripts/fetch-x-mentions.mjs \
//     alpha '(@alpha OR to:alpha OR "alpha app" OR alpha.family) -filter:nativeretweets' 2024-12-13 2026-09-21
//
// Accounts come from the secrets-manager store (~/.config/secrets-manager/secrets.sqlite): active X rows,
// each its own X rate bucket, so N accounts ~= N x throughput. SECRETS_DB overrides the path.
// Config (the calling script's .env):
//   RESIDENTIAL_PROXY_URL Proxy every request routes through (or X_PROXY_URLS).
//   X_PROXY_URLS          Comma-separated, aligned to the store accounts by row order (one exit each).
//   X_SEARCH_QUERY_ID / X_BEARER / X_TID_*  anti-bot ingredients; refresh if X starts returning 404.
//
// tweets.jsonl holds one tweet per line; each has a `sources` array of which query term(s)
// surfaced it (@handle / to:handle / "phrase" / domain). Fetched days are appended as they
// finish; a run that reaches its end rewrites the file deduplicated by id, newest first.
// tweets.out.json is the progress file and the only input to planning:
//   { query, dailyLimit, days: { "YYYY-MM-DD": { count, oldest, refill } } }
// count is how many tweets the day holds, oldest the ISO time of its oldest one, refill (only
// on days that were refilled) how many times. A run fetches the days of its range missing from
// the file, plus the gap days with --refill.
import { realpathSync } from "node:fs";

import { appendFile, readFile, writeFile, mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import { homedir } from "node:os";
import { createHash } from "node:crypto";
import { DatabaseSync } from "node:sqlite";
const isMain = process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) loadEnvFile(new URL(".env", import.meta.url));

let ProxyAgent;
try { ({ ProxyAgent } = await import("undici")); }
catch (e) {
  if (e.code !== 'ERR_MODULE_NOT_FOUND') throw e;
  throw new Error('Install dependencies: npm install --prefix "${CLAUDE_PLUGIN_ROOT}/skills/fetch-x-mentions/scripts"', {cause:e});
}

// ---- x-client-transaction-id header, required on X GraphQL/REST API calls ----
//
// The ingredients are captured from a real x.com browser session and go stale on X redeploy:
//   X_TID_VERIFICATION  <meta name="twitter-site-verification"> content
//   X_TID_FRAME         'd' of the SELECTED loading-x-anim frame (frames[keyBytes[5]%4], 2nd path)
//   X_TID_ROW           first int from ondemand.s "(n[NN],16)" matches
//   X_TID_INDICES       the remaining ints from those matches (comma-separated)
// The math below mirrors X's client (as implemented by the x-client-transaction-id project).

const KEYWORD = "obfiowerehiring";
const ADDITIONAL_RANDOM = 3;
const EPOCH = 1682924400000;
const TOTAL_TIME = 4096;

const isOdd = (n) => (n % 2 ? -1.0 : 0.0);

function solve(value, minV, maxV, rounding) {
  const result = (value * (maxV - minV)) / 255 + minV;
  return rounding ? Math.floor(result) : Math.round(result * 100) / 100;
}

function interpolate(fromList, toList, f) {
  return fromList.map((v, i) => v * (1 - f) + toList[i] * f);
}

function convertRotationToMatrix(deg) {
  const rad = (deg * Math.PI) / 180;
  return [Math.cos(rad), -Math.sin(rad), Math.sin(rad), Math.cos(rad)];
}

function floatToHex(x) {
  const result = [];
  let quotient = Math.floor(x);
  const fraction = x - quotient;
  while (quotient > 0) {
    quotient = Math.floor(x / 16);
    const remainder = Math.floor(x - quotient * 16);
    result.unshift(
      remainder > 9
        ? String.fromCharCode(remainder + 55)
        : remainder.toString(),
    );
    x = quotient;
  }
  if (fraction === 0) return result.join("");
  result.push(".");
  let frac = fraction;
  while (frac > 0) {
    frac *= 16;
    const integer = Math.floor(frac);
    frac -= integer;
    result.push(
      integer > 9 ? String.fromCharCode(integer + 55) : integer.toString(),
    );
  }
  return result.join("");
}

// Cubic bezier easing: solve y at x=time over control points [x1,y1,x2,y2].
function cubicCalc(a, b, m) {
  return (
    3.0 * a * (1 - m) * (1 - m) * m + 3.0 * b * (1 - m) * m * m + m * m * m
  );
}
function cubicValue(curves, time) {
  let startGradient = 0,
    endGradient = 0,
    start = 0.0,
    mid = 0.0,
    end = 1.0;
  if (time <= 0.0) {
    if (curves[0] > 0.0) startGradient = curves[1] / curves[0];
    else if (curves[1] === 0.0 && curves[2] > 0.0)
      startGradient = curves[3] / curves[2];
    return startGradient * time;
  }
  if (time >= 1.0) {
    if (curves[2] < 1.0) endGradient = (curves[3] - 1.0) / (curves[2] - 1.0);
    else if (curves[2] === 1.0 && curves[0] < 1.0)
      endGradient = (curves[1] - 1.0) / (curves[0] - 1.0);
    return 1.0 + endGradient * (time - 1.0);
  }
  while (start < end) {
    mid = (start + end) / 2;
    const xEst = cubicCalc(curves[0], curves[2], mid);
    if (Math.abs(time - xEst) < 0.00001)
      return cubicCalc(curves[1], curves[3], mid);
    if (xEst < time) start = mid;
    else end = mid;
  }
  return cubicCalc(curves[1], curves[3], mid);
}

function animate(frameRow, targetTime) {
  const fromColor = frameRow.slice(0, 3).concat(1).map(Number);
  const toColor = frameRow.slice(3, 6).concat(1).map(Number);
  const fromRotation = [0.0];
  const toRotation = [solve(frameRow[6], 60.0, 360.0, true)];
  const curves = frameRow
    .slice(7)
    .map((item, counter) => solve(item, isOdd(counter), 1.0, false));
  const val = cubicValue(curves, targetTime);
  const color = interpolate(fromColor, toColor, val).map((v) =>
    v > 0 ? v : 0,
  );
  const rotation = interpolate(fromRotation, toRotation, val);
  const matrix = convertRotationToMatrix(rotation[0]);
  const strArr = color.slice(0, -1).map((v) => Math.round(v).toString(16));
  for (const value of matrix) {
    let rounded = Math.round(value * 100) / 100;
    if (rounded < 0) rounded = -rounded;
    const hexValue = floatToHex(rounded);
    strArr.push(
      hexValue.startsWith(".") ? `0${hexValue}`.toLowerCase() : hexValue || "0",
    );
  }
  strArr.push("0", "0");
  return strArr.join("").replace(/[.-]/g, "");
}

let cache = null;
function init() {
  if (cache) return cache;
  const VERIFICATION = requireEnv("X_TID_VERIFICATION");
  const FRAME_D = requireEnv("X_TID_FRAME");
  const ROW_INDEX = Number(requireEnv("X_TID_ROW"));
  const KEY_BYTE_INDICES = requireEnv("X_TID_INDICES")
    .split(",")
    .map(Number)
    .filter((n) => Number.isInteger(n));
  const missing = [];
  if (!VERIFICATION) missing.push("X_TID_VERIFICATION");
  if (!FRAME_D) missing.push("X_TID_FRAME");
  if (!Number.isInteger(ROW_INDEX)) missing.push("X_TID_ROW");
  if (!KEY_BYTE_INDICES.length) missing.push("X_TID_INDICES");
  if (missing.length) {
    throw new Error(
      `x-client-transaction-id needs ${missing.join(", ")} in the calling script's .env ` +
        `(re-extract from x.com when X redeploys).`,
    );
  }

  const keyBytes = [...Buffer.from(VERIFICATION, "base64")];
  const arr = FRAME_D.substring(9)
    .split("C")
    .map((item) => {
      const cleaned = item.replace(/[^\d]+/g, " ").trim();
      return cleaned === ""
        ? []
        : cleaned.split(/\s+/).map((s) => parseInt(s, 10));
    });
  const rowIndex = keyBytes[ROW_INDEX] % 16;
  let frameTime = KEY_BYTE_INDICES.reduce((a, i) => a * (keyBytes[i] % 16), 1);
  frameTime = Math.round(frameTime / 10) * 10;
  const frameRow = arr[rowIndex];
  if (!frameRow) {
    throw new Error(
      `X_TID_FRAME has no row ${rowIndex}; re-extract the selected animation frame.`,
    );
  }
  cache = { keyBytes, animationKey: animate(frameRow, frameTime / TOTAL_TIME) };
  return cache;
}

export function generateTransactionId(method, path) {
  const { keyBytes, animationKey } = init();
  const timeNow = Math.floor((Date.now() - EPOCH) / 1000);
  const timeBytes = [
    timeNow & 0xff,
    (timeNow >> 8) & 0xff,
    (timeNow >> 16) & 0xff,
    (timeNow >> 24) & 0xff,
  ];
  const hashBytes = [
    ...createHash("sha256")
      .update(`${method}!${path}!${timeNow}${KEYWORD}${animationKey}`)
      .digest(),
  ];
  const rnd = Math.floor(Math.random() * 256);
  const bytesArr = [
    ...keyBytes,
    ...timeBytes,
    ...hashBytes.slice(0, 16),
    ADDITIONAL_RANDOM,
  ];
  return Buffer.from([rnd, ...bytesArr.map((b) => b ^ rnd)])
    .toString("base64")
    .replace(/=+$/, "");
}

const GQL = "https://x.com/i/api/graphql";
// From the calling script's .env — no hardcoded fallbacks. QUERY_ID rotates with X's web bundle;
// BEARER is X's public web bearer.
const UA =
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 " +
  "(KHTML, like Gecko) Version/17.0 Safari/605.1.15";

const PAGE = 20; // tweets per SearchTimeline page
const DEFAULT_DAILY_LIMIT = 1000; // = 50 pages, one account's 15-minute search quota
const REQUEST_SPACING_MS = 800;
const MAX_RETRIES = 6;

// The exact featureSwitches SearchTimeline declares in the current web bundle. X rejects a
// request (400) if any required feature is absent, so all must be present; extracted from
// main.<hash>.js. Re-extract if X starts 400ing with "features cannot be null".
const FEATURES = {
  rweb_video_screen_enabled: false,
  rweb_cashtags_enabled: true,
  profile_label_improvements_pcf_label_in_post_enabled: true,
  responsive_web_profile_redirect_enabled: true,
  rweb_tipjar_consumption_enabled: true,
  verified_phone_label_enabled: false,
  creator_subscriptions_tweet_preview_api_enabled: true,
  responsive_web_graphql_timeline_navigation_enabled: true,
  premium_content_api_read_enabled: false,
  communities_web_enable_tweet_community_results_fetch: true,
  c9s_tweet_anatomy_moderator_badge_enabled: true,
  responsive_web_grok_analyze_button_fetch_trends_enabled: false,
  responsive_web_grok_analyze_post_followups_enabled: true,
  rweb_cashtags_composer_attachment_enabled: true,
  responsive_web_jetfuel_frame: true,
  rweb_sports_post_context_enabled: true,
  responsive_web_grok_share_attachment_enabled: true,
  responsive_web_grok_annotations_enabled: true,
  articles_preview_enabled: true,
  responsive_web_edit_tweet_api_enabled: true,
  rweb_conversational_replies_downvote_enabled: false,
  graphql_is_translatable_rweb_tweet_is_translatable_enabled: true,
  view_counts_everywhere_api_enabled: true,
  longform_notetweets_consumption_enabled: true,
  responsive_web_twitter_article_tweet_consumption_enabled: true,
  content_disclosure_indicator_enabled: true,
  content_disclosure_ai_generated_indicator_enabled: true,
  responsive_web_grok_show_grok_translated_post: false,
  responsive_web_grok_analysis_button_from_backend: true,
  post_ctas_fetch_enabled: true,
  freedom_of_speech_not_reach_fetch_enabled: true,
  standardized_nudges_misinfo: true,
  tweet_with_visibility_results_prefer_gql_limited_actions_policy_enabled: true,
  longform_notetweets_rich_text_read_enabled: true,
  longform_notetweets_inline_media_enabled: true,
  responsive_web_nested_quote_preview_enabled: true,
  responsive_web_grok_image_annotation_enabled: true,
  responsive_web_grok_imagine_annotation_enabled: true,
  responsive_web_grok_community_note_auto_translation_is_enabled: false,
  responsive_web_enhance_cards_enabled: false,
};

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// A wait for the log: "13m 24s", or "45s" under a minute.
export function duration(ms) {
  const s = Math.round(ms / 1000);
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${s % 60}s`;
}

function makeDispatcher(url) {
  if (!url) {
    throw new Error(
      "No proxy set. Add RESIDENTIAL_PROXY_URL (or X_PROXY_URLS) to the calling script's .env.",
    );
  }
  const u = new URL(url);
  const token =
    u.username &&
    `Basic ${Buffer.from(`${decodeURIComponent(u.username)}:${decodeURIComponent(u.password)}`).toString("base64")}`;
  const uri = `${u.protocol}//${u.host}`;
  return new ProxyAgent(token ? { uri, token } : uri);
}

function proxyList() {
  const proxies = (process.env.X_PROXY_URLS || "")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
  const fallbackProxy = process.env.RESIDENTIAL_PROXY_URL || process.env.HTTPS_PROXY;
  return { proxies, fallbackProxy };
}

// The credential store secrets-manager fills: active X accounts with a live auth_token + ct0.
// Returns null when the store or its table is absent, so loadAccounts can fall back to the env.
function loadAccountsFromStore() {
  const stateDir = process.env.SECRETS_MANAGER_STATE_PATH || join(homedir(), ".config", "secrets-manager");
  const path = process.env.SECRETS_DB || join(stateDir, "secrets.sqlite");
  let rows;
  try {
    const db = new DatabaseSync(path, { readOnly: true });
    rows = db
      .prepare(
        "SELECT username, auth_token, ct0 FROM x WHERE status = 'active' " +
          "AND auth_token IS NOT NULL AND auth_token <> '' " +
          "AND ct0 IS NOT NULL AND ct0 <> '' ORDER BY username",
      )
      .all();
    db.close();
  } catch {
    return null;
  }
  if (!rows.length) return null;
  const { proxies, fallbackProxy } = proxyList();
  return rows.map((r, i) => ({
    label: r.username,
    authToken: r.auth_token,
    ct0: r.ct0,
    dispatcher: makeDispatcher(proxies[i] || proxies[0] || fallbackProxy),
  }));
}

// One account: { label, authToken, ct0, dispatcher }; the label prefixes its log lines. The single
// source of accounts is the secrets-manager store (~/.config/secrets-manager/secrets.sqlite); SECRETS_DB
// overrides its path.
export function loadAccounts() {
  const accounts = loadAccountsFromStore();
  if (!accounts) {
    throw new Error(
      "No active X accounts in the store (~/.config/secrets-manager/secrets.sqlite). Import credential " +
        "files and run intel fetch-x-mentions/scripts/verify-x.mjs first.",
    );
  }
  return accounts;
}

// Confirms an { authToken, ct0, dispatcher } pair authenticates, via one live SearchTimeline call.
export async function verifyAuth(acct) {
  try {
    const { status, body } = await getPage(
      searchUrl("twitter"),
      acct,
      undefined,
      () => Promise.resolve(),
      () => {},
    );
    return status === 200 && !!body?.data?.search_by_raw_query;
  } catch {
    return false;
  }
}

// Turns a vendor auth_token (no ct0) into a usable pair: X sets a session ct0 for a valid token on
// any request, then verifyAuth confirms the pair actually authenticates. Returns { ok, ct0 }.
export async function provisionPair(authToken, dispatcher) {
  const boot = await fetch(searchUrl("twitter"), {
    dispatcher,
    headers: { authorization: `Bearer ${requireEnv("X_BEARER")}`, cookie: `auth_token=${authToken}`, "User-Agent": UA },
  });
  await boot.text();
  let ct0;
  for (const c of boot.headers.getSetCookie?.() ?? []) {
    const pair = c.split(";")[0];
    if (pair.startsWith("ct0=")) ct0 = pair.slice(4);
  }
  if (!ct0) return { ok: false };
  const ok = await verifyAuth({ label: "verify", authToken, ct0, dispatcher });
  return { ok, ct0 };
}

const searchPath = () => `/i/api/graphql/${requireEnv("X_SEARCH_QUERY_ID")}/SearchTimeline`;

function headers(acct) {
  return {
    authorization: `Bearer ${requireEnv("X_BEARER")}`,
    "x-csrf-token": acct.ct0,
    cookie: `auth_token=${acct.authToken}; ct0=${acct.ct0}`,
    "x-twitter-auth-type": "OAuth2Session",
    "x-twitter-active-user": "yes",
    "x-twitter-client-language": "en",
    "x-client-transaction-id": generateTransactionId("GET", searchPath()),
    "content-type": "application/json",
    "User-Agent": UA,
    Referer: "https://x.com/search",
    "x-twitter-client-version": "web",
  };
}

export function searchUrl(query, cursor) {
  requireEnv("X_SEARCH_QUERY_ID");
  const variables = {
    rawQuery: query,
    count: PAGE,
    querySource: "typed_query",
    product: "Latest", // chronological — the exhaustive view, not "Top"
  };
  if (cursor) variables.cursor = cursor;
  const params = new URLSearchParams({
    variables: JSON.stringify(variables),
    features: JSON.stringify(FEATURES),
  });
  return `${GQL}/${requireEnv("X_SEARCH_QUERY_ID")}/SearchTimeline?${params.toString()}`;
}

// Parse the query into labelled matchers so each tweet can record which term(s) surfaced it.
function parseQueryTerms(query) {
  const terms = [];
  for (const m of query.matchAll(/"([^"]+)"/g))
    terms.push({
      label: `"${m[1]}"`,
      kind: "phrase",
      value: m[1].toLowerCase(),
    });
  for (const m of query.matchAll(/\bto:(\w+)/gi))
    terms.push({
      label: `to:${m[1]}`,
      kind: "reply",
      value: m[1].toLowerCase(),
    });
  for (const m of query.matchAll(/(?:^|[\s(])@(\w+)/g))
    terms.push({
      label: `@${m[1]}`,
      kind: "mention",
      value: m[1].toLowerCase(),
    });
  const stripped = query.replace(/"[^"]*"/g, " ").replace(/-?\w+:\S+/g, " ");
  for (const m of stripped.matchAll(/\b([a-z0-9-]+\.[a-z0-9.]+)\b/gi))
    terms.push({ label: m[1], kind: "domain", value: m[1].toLowerCase() });
  const seen = new Set();
  return terms.filter((t) => !seen.has(t.label) && seen.add(t.label));
}

// Which query terms a tweet satisfies, from its text + metadata.
function matchSources(terms, { text, mentions, replyTo, urls }) {
  const t = (text || "").toLowerCase();
  const men = (mentions || []).map((s) => s.toLowerCase());
  const rep = (replyTo || "").toLowerCase();
  const urlStr = (urls || []).join(" ").toLowerCase();
  const out = [];
  for (const term of terms) {
    let hit = false;
    if (term.kind === "phrase") hit = t.includes(term.value);
    else if (term.kind === "mention")
      hit = men.includes(term.value) || t.includes("@" + term.value);
    else if (term.kind === "reply") hit = rep === term.value;
    else if (term.kind === "domain")
      hit = t.includes(term.value) || urlStr.includes(term.value);
    if (hit) out.push(term.label);
  }
  return out;
}

// Pull tweets + the bottom cursor out of a SearchTimeline response.
export function parsePage(body, terms) {
  const tweets = [];
  let cursor = null;
  const instructions =
    body?.data?.search_by_raw_query?.search_timeline?.timeline?.instructions ??
    [];
  for (const ins of instructions) {
    const entries = [...(ins.entries ?? []), ...(ins.entry ? [ins.entry] : [])];
    for (const entry of entries) {
      const eid = entry.entryId ?? "";
      if (eid.startsWith("cursor-bottom-")) {
        cursor = entry.content?.value ?? null;
        continue;
      }
      const result =
        entry.content?.itemContent?.tweet_results?.result ??
        entry.content?.itemContent?.tweet_results?.result?.tweet;
      const t = result?.tweet ?? result;
      if (!t || !t.legacy) continue;
      const u = t.core?.user_results?.result;
      const ul = u?.legacy ?? {};
      const text =
        t.legacy.full_text ??
        t.note_tweet?.note_tweet_results?.result?.text ??
        "";
      const mentions = (t.legacy.entities?.user_mentions ?? []).map(
        (m) => m.screen_name,
      );
      const urls = (t.legacy.entities?.urls ?? [])
        .map((x) => x.expanded_url ?? x.url)
        .filter(Boolean);
      tweets.push({
        id: t.rest_id ?? t.legacy.id_str,
        author: ul.screen_name ?? u?.core?.screen_name ?? null,
        author_name: ul.name ?? u?.core?.name ?? null,
        author_followers: ul.followers_count ?? null,
        author_verified: u?.is_blue_verified ?? ul.verified ?? null,
        text,
        created_at: t.legacy.created_at ?? null,
        likes: t.legacy.favorite_count ?? 0,
        retweets: t.legacy.retweet_count ?? 0,
        replies: t.legacy.reply_count ?? 0,
        quotes: t.legacy.quote_count ?? 0,
        views: t.views?.count ?? null,
        lang: t.legacy.lang ?? null,
        url: `https://x.com/${ul.screen_name ?? "i"}/status/${t.rest_id ?? t.legacy.id_str}`,
        sources: matchSources(terms, {
          text,
          mentions,
          replyTo: t.legacy.in_reply_to_screen_name ?? null,
          urls,
        }),
      });
    }
  }
  return { tweets, cursor };
}

export const fetchAs = (url, acct) =>
  fetch(url, { dispatcher: acct.dispatcher, headers: headers(acct) });

// One search page. A 429 always waits for X's own reset and continues — X tells us how long to
// wait, so we never give an account up for a 429. Every other retryable failure (proxy error,
// truncated body, 5xx, empty 404) is logged and waits 1s, 2s, 4s ... 32s, and the request is
// given up after MAX_RETRIES of them. A 4xx with a body is a broken request (credentials,
// queryId, features), not a transient, and is raised at once.
export async function getPage(
  url,
  acct,
  doFetch = fetchAs,
  wait = sleep,
  log = (line) => process.stderr.write(line + "\n"),
) {
  let attempt = 0;
  const retry = async (why) => {
    const waitMs = 1000 * 2 ** attempt;
    attempt++;
    log(`  [${acct.label}] ${why}`);
    await wait(waitMs);
  };
  while (attempt < MAX_RETRIES) {
    let res;
    try {
      res = await doFetch(url, acct);
    } catch (e) {
      await retry(e.code ?? e.cause?.code ?? e.name ?? "fetch error");
      continue;
    }
    if (res.status === 200) {
      try {
        return { status: 200, body: await res.json() };
      } catch {
        // The proxy can close an HTTP/2 stream after headers but before the body is complete.
        await retry("200 with a bad body");
        continue;
      }
    }
    if (res.status === 429) {
      // Per-account quota: wait until exactly X's own reset instant, then retry — no padding, no
      // give-up. waitMs is the time left until reset, so setTimeout resumes at or after it
      // (setTimeout never fires early), never before: no race against the required wait. A reset
      // already past means the window has reopened, so retry at once; a missing header waits 60s.
      const reset = Number(res.headers.get("x-rate-limit-reset"));
      const waitMs = reset ? Math.max(0, reset * 1000 - Date.now()) : 60000;
      await res.body?.cancel?.();
      log(`  [${acct.label}] 429 — ${duration(waitMs)}`);
      await wait(waitMs);
      continue;
    }
    const text = await res.text().catch(() => "");
    // An HTML body on a 4xx is an edge/proxy block (Cloudflare, a bot wall), not X's API: the
    // request never reached X. It is transient and tied to the proxy exit, so retry — the next
    // attempt routes through a fresh residential exit. X's own API errors are JSON.
    const isEdgeBlock = /^\s*<(?:!doctype|html)/i.test(text);
    if (
      res.status >= 400 &&
      res.status < 500 &&
      !isEdgeBlock &&
      !(res.status === 404 && text.trim() === "")
    ) {
      // A JSON 4xx is a broken request (queryId/features drift, credentials) — surface it,
      // don't silently loop.
      throw new Error(
        `SearchTimeline ${res.status}: ${text.slice(0, 300)}\n` +
          "If this mentions the operation/features, update X_SEARCH_QUERY_ID / FEATURES.",
      );
    }
    // A transient: an empty 404, an edge/proxy block, or a 5xx. Retry through a fresh exit.
    await retry(isEdgeBlock ? `${res.status} edge block (html)` : String(res.status));
  }
  throw new Error(`SearchTimeline failed after ${MAX_RETRIES} attempts`);
}

const instant = (iso) =>
  new Date(iso).toISOString().slice(0, 19).replace("T", "_") + "_UTC";

const nextDay = (day) => {
  const d = new Date(day + "T00:00:00Z");
  d.setUTCDate(d.getUTCDate() + 1);
  return d.toISOString().slice(0, 10);
};

// Where a refill of a day starts: one second past its oldest held tweet, so the boundary tweet
// comes back too (a duplicate line the end-of-run sort removes) and nothing between is skipped.
export function untilInstant(oldestIso) {
  return instant(new Date(Date.parse(oldestIso) + 1000).toISOString());
}

// The search query for one UTC day, or for the part of it before `until`. Bare
// `since:YYYY-MM-DD until:YYYY-MM-DD` is interpreted by X as an inclusive, Pacific-time window
// (~2 days wide, drifting up to 2 days past `until`), so the window is pinned to UTC instants.
export function dayQuery(baseQuery, day, until = null) {
  return `${baseQuery} since:${day}_00:00:00_UTC until:${until ?? `${nextDay(day)}_00:00:00_UTC`}`;
}

// Fetch one day (all cursor pages) for one account: every tweet X returned, deduplicated.
export async function fetchDay(
  acct,
  baseQuery,
  day,
  until,
  terms,
  maxPages,
  fetchPage = getPage,
  wait = sleep,
) {
  const query = dayQuery(baseQuery, day, until);
  const seen = new Set();
  const dayTweets = [];
  let cursor = null;
  for (let page = 0; page < maxPages; page++) {
    const { status, body } = await fetchPage(searchUrl(query, cursor), acct);
    if (status !== 200 || !body) return dayTweets;
    const { tweets, cursor: next } = parsePage(body, terms);
    for (const tw of tweets) {
      if (!tw.id || seen.has(tw.id)) continue;
      seen.add(tw.id);
      dayTweets.push(tw);
    }
    // Stop when the page had no tweets (the end of the day, or X throttling), or no cursor.
    if (tweets.length === 0 || !next || next === cursor) break;
    cursor = next;
    await wait(REQUEST_SPACING_MS);
  }
  return dayTweets;
}

function* eachDay(since, until) {
  for (let d = since; d < until; d = nextDay(d)) yield d;
}

async function loadJson(path, fallback) {
  try {
    return JSON.parse(await readFile(path, "utf8"));
  } catch {
    return fallback;
  }
}

// Split argv into positionals, the --daily-limit cap (also as a page count) and --refill [n]
// (null when absent; n defaults to 0).
export function parseArgs(argv) {
  const positional = [];
  let dailyLimit = DEFAULT_DAILY_LIMIT;
  let refill = null;
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--daily-limit" || a.startsWith("--daily-limit=")) {
      const raw = a.includes("=") ? a.slice(a.indexOf("=") + 1) : argv[++i];
      dailyLimit = Number(raw);
      if (!Number.isInteger(dailyLimit) || dailyLimit < 1) {
        throw new Error(`--daily-limit needs a positive integer, got "${raw}"`);
      }
    } else if (a === "--refill") {
      refill = /^\d+$/.test(argv[i + 1] ?? "") ? Number(argv[++i]) : 0;
    } else if (a.startsWith("--refill=")) {
      refill = Number(a.slice("--refill=".length));
      if (!Number.isInteger(refill) || refill < 0) {
        throw new Error(`--refill needs a non-negative integer, got "${a}"`);
      }
    } else if (a.startsWith("--")) throw new Error(`Unknown option ${a}`);
    else positional.push(a);
  }
  return {
    positional,
    dailyLimit,
    maxPages: Math.ceil(dailyLimit / PAGE),
    refill,
  };
}

// The progress file for this query and limit: the saved one when it matches, a fresh one when
// there is none. A saved file for another query or limit is refused rather than mixed.
export function openProgress(saved, query, dailyLimit) {
  if (saved == null) return { query, dailyLimit, days: {} };
  if (typeof saved.days !== "object" || saved.days === null) {
    throw new Error("tweets.out.json has no days map");
  }
  if (saved.query !== query) {
    throw new Error(`tweets.out.json is for another query: ${saved.query}`);
  }
  if (saved.dailyLimit !== dailyLimit) {
    throw new Error(
      `tweets.out.json is for dailyLimit ${saved.dailyLimit}, not ${dailyLimit}`,
    );
  }
  return saved;
}

// A day the fetch did not finish: nothing came back, or paging stopped exactly on a page
// boundary short of the limit, which is how an empty page in the middle of a day looks.
export function isGap({ count }, dailyLimit) {
  return count === 0 || (count % PAGE === 0 && count < dailyLimit);
}

// The days of [since, until) this run fetches, newest first: those missing from the progress
// file, plus with `refill` (a number) the gap days refilled at most that many times, each from
// its oldest held tweet backwards.
export function planDays(progress, since, until, refill) {
  const plan = [];
  for (const day of eachDay(since, until)) {
    const held = progress.days[day];
    if (!held) plan.push({ day, until: null });
    else if (
      refill !== null &&
      isGap(held, progress.dailyLimit) &&
      (held.refill ?? 0) <= refill
    ) {
      plan.push({ day, until: held.oldest ? untilInstant(held.oldest) : null });
    }
  }
  return plan.reverse();
}

const isoOf = (t) => new Date(t.created_at).toISOString();

// The progress entry for a day after a fetch: a first fetch records what came back; a refill
// adds only the tweets older than the day's previous oldest one and counts itself in `refill`.
export function mergeDay(held, got, refilled) {
  let entry;
  if (got.length === 0) entry = { ...(held ?? { count: 0 }) };
  else {
    const times = got.map(isoOf).sort();
    if (!held?.oldest) entry = { count: got.length, oldest: times[0] };
    else {
      const older = times.filter((t) => t < held.oldest).length;
      entry = {
        count: held.count + older,
        oldest: times[0] < held.oldest ? times[0] : held.oldest,
      };
    }
  }
  if (refilled) entry.refill = (held?.refill ?? 0) + 1;
  return entry;
}

// The log deduplicated by id (last line wins) and ordered newest first.
export function sortTweets(tweets) {
  const byId = new Map(tweets.map((t) => [t.id, t]));
  return [...byId.values()].sort(
    (a, b) => Date.parse(b.created_at) - Date.parse(a.created_at),
  );
}

// Every account pulls the next day off one queue until it is empty. An account whose fetch
// throws is paused for the rest of the run and its day goes back on the queue for the others;
// the run ends when the queue is drained or every account is paused. Returns the paused labels
// and how many days were left unfetched.
export async function drainPlan(accounts, plan, fetchEntry, wait, log) {
  const queue = [...plan];
  const paused = [];
  await Promise.all(
    accounts.map(async (acct) => {
      while (queue.length) {
        const entry = queue.shift();
        try {
          await fetchEntry(acct, entry);
        } catch (e) {
          queue.push(entry);
          paused.push(acct.label);
          log(`  [${acct.label}] ${entry.day}: ${e.message}`);
          log(`  [${acct.label}] paused for the rest of the run`);
          return;
        }
        await wait(REQUEST_SPACING_MS);
      }
    }),
  );
  return { paused, left: queue.length };
}

async function main() {
  const { positional, dailyLimit, maxPages, refill } = parseArgs(
    process.argv.slice(2),
  );
  const [slug, query, sinceArg, untilArg] = positional;
  if (!slug || !query) {
    console.error(
      'Usage: node ${CLAUDE_PLUGIN_ROOT}/skills/fetch-x-mentions/scripts/fetch-x-mentions.mjs <slug> "<query>" [since] [until] [--daily-limit <n>] [--refill [<n>]]',
    );
    process.exit(1);
  }
  const since = sinceArg || "2025-09-20";
  const until = untilArg || new Date().toISOString().slice(0, 10);
  const terms = parseQueryTerms(query);

  requireEnv("X_BEARER");
  requireEnv("X_SEARCH_QUERY_ID");
  const accounts = loadAccounts();
  const outDir = join("docs", "intel", "x", slug); // run from the repo root
  await mkdir(outDir, { recursive: true });
  const logPath = join(outDir, "tweets.jsonl");
  const progressPath = join(outDir, "tweets.out.json");

  const progress = openProgress(
    await loadJson(progressPath, null),
    query,
    dailyLimit,
  );
  const plan = planDays(progress, since, until, refill);
  const total = () =>
    Object.values(progress.days).reduce((n, d) => n + d.count, 0);
  process.stderr.write(
    `${slug}: ${plan.length} days to fetch across ${accounts.length} account(s), ` +
      `up to ${maxPages * PAGE} tweets/day\n`,
  );

  // Lanes finish days concurrently; the append and the progress write of each day go through
  // one chain so lines never interleave and the progress file is never torn.
  let checkpoint = Promise.resolve();
  const commitDay = (day, got) => {
    checkpoint = checkpoint.then(async () => {
      if (got.length) {
        await appendFile(
          logPath,
          got.map((t) => JSON.stringify(t)).join("\n") + "\n",
        );
      }
      progress.days[day] = mergeDay(
        progress.days[day],
        got,
        day in progress.days,
      );
      await writeFile(progressPath, JSON.stringify(progress, null, 1));
    });
    return checkpoint;
  };

  const { paused, left } = await drainPlan(
    accounts,
    plan,
    async (acct, { day, until: fillUntil }) => {
      const got = await fetchDay(acct, query, day, fillUntil, terms, maxPages);
      await commitDay(day, got);
      process.stderr.write(
        `  [${acct.label}] ${day}: +${got.length} (total ${total()})\n`,
      );
    },
    sleep,
    (line) => process.stderr.write(line + "\n"),
  );
  if (paused.length) {
    process.stderr.write(
      `${slug}: paused ${paused.join(", ")}; ${left} day(s) left unfetched\n`,
    );
  }
  if (left) process.exitCode = 1;

  // The run reached its end: rewrite the log deduplicated and newest first.
  const lines = (await readFile(logPath, "utf8").catch(() => ""))
    .split("\n")
    .filter(Boolean);
  const sorted = sortTweets(lines.map((l) => JSON.parse(l)));
  await writeFile(
    logPath,
    sorted.map((t) => JSON.stringify(t)).join("\n") + "\n",
  );
  console.log(
    `${slug}: ${sorted.length} tweets across ${Object.keys(progress.days).length} days -> ${logPath}`,
  );
}

if (isMain) await main();
