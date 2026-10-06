import { requireEnv, loadEnvFile } from "../../fetch-x-mentions/scripts/env.mjs";
// ABOUTME: Searches X with the account pool of the secrets-manager store: one SearchTimeline query,
// ABOUTME: printed as JSON lines; accounts rotate machine-wide through a shared state file.
//
// Usage (from any directory; many processes may run at once):
//   node ${CLAUDE_PLUGIN_ROOT}/skills/fetch-x-posts/scripts/fetch-x-posts.mjs "<query>" [--limit 40] [--latest|--top]
//   The query goes to X as written, so operators work (from:user since:2021-01-01_00:00:00_UTC
//   min_faves:1000 filter:replies). --latest (the default) is the chronological view, --top X's ranked one.
//
// stdout: one JSON object per post: id, url, created_at (UTC), user, name, text, likes, retweets,
// replies, views, quoted (url or null), in_reply_to (id or null).
// Exit 0 with no lines only when X returned nothing on 3 accounts; any failure exits 1 with one
// `fetch-x-posts: <reason>` line on stderr (posts found before the failure are still printed).
//
// The request itself (headers, x-client-transaction-id, query id, bearer, proxy, accounts) is the
// fetch-x-mentions client, configured through ~/.config/intel/.env. This file adds the rotation: the state
// in ~/.cache/case-study-limits/fetch-x-posts.sqlite holds, per account, when its next request may start
// and until when it is paused, plus a log of every request (table `requests`).

import { mkdirSync, realpathSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { homedir } from "node:os";
import { DatabaseSync } from "node:sqlite";

const isMain = process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) loadEnvFile();

const STATE_PATH = process.env.INTEL_CASE_STUDY_DIR
  ? join(process.env.INTEL_CASE_STUDY_DIR, "cache", "fetch-x-posts.sqlite")
  : join(homedir(), ".cache", "case-study-limits", "fetch-x-posts.sqlite");

const DEFAULT_LIMIT = 40;
export const GAP_MS = 3200; // between two requests on one account: the 3 seconds asked for, plus margin
const RATE_LIMIT_PAUSE_MS = 15 * 60 * 1000; // a 429 without x-rate-limit-reset
const BAD_PAUSE_MS = 6 * 60 * 60 * 1000; // a 401/403: long enough to skip, short enough to heal after a re-login
const MAX_WAIT_MS = 60 * 1000; // all the waiting one run may do for accounts to come free
const REQUEST_TIMEOUT_MS = 30 * 1000;
const EMPTY_ACCOUNTS = 3; // accounts that must answer an empty page before it counts as the end
const MAX_RETRIES = 4; // transient failures (proxy error, 5xx, edge block) one run tolerates
const SMALL_POOL = 10; // below this many usable accounts the run says so
const STORE_TRIES = 5;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const clock = (ms) => new Date(ms).toISOString().slice(11, 19) + " UTC";

// Split argv into the query, --limit and the search product.
export function parseArgs(argv) {
  let query = null;
  let limit = DEFAULT_LIMIT;
  let product = "Latest";
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--limit") {
      const raw = argv[++i];
      limit = Number(raw);
      if (!Number.isInteger(limit) || limit < 1) {
        throw new Error(`--limit needs a positive integer, got "${raw}"`);
      }
    } else if (a === "--latest") product = "Latest";
    else if (a === "--top") product = "Top";
    else if (a.startsWith("--")) throw new Error(`unknown option ${a}`);
    else if (query === null) query = a;
    else throw new Error("the query must be one quoted argument");
  }
  if (!query) throw new Error('usage: fetch-x-posts.mjs "<query>" [--limit 40] [--latest|--top]');
  return { query, limit, product };
}

// A post's whole text: the long-post body when there is one (full_text cuts it), and for a
// retweet the original's text (full_text cuts that too).
function textOf(t) {
  const retweeted = t.legacy?.retweeted_status_result?.result;
  const original = retweeted?.tweet ?? retweeted;
  if (original?.legacy) {
    const u = original.core?.user_results?.result;
    return `RT @${u?.core?.screen_name ?? u?.legacy?.screen_name}: ${textOf(original)}`;
  }
  const text = t.note_tweet?.note_tweet_results?.result?.text ?? t.legacy?.full_text ?? "";
  return text.replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");
}

function toRow(t) {
  const u = t.core?.user_results?.result;
  const user = u?.core?.screen_name ?? u?.legacy?.screen_name ?? null;
  const id = t.rest_id ?? t.legacy.id_str;
  const created = Date.parse(t.legacy.created_at);
  const quoted = t.legacy.quoted_status_permalink?.expanded;
  return {
    id,
    url: `https://x.com/${user ?? "i"}/status/${id}`,
    created_at: Number.isNaN(created) ? null : new Date(created).toISOString(),
    user,
    name: u?.core?.name ?? u?.legacy?.name ?? null,
    text: textOf(t),
    likes: t.legacy.favorite_count ?? 0,
    retweets: t.legacy.retweet_count ?? 0,
    replies: t.legacy.reply_count ?? 0,
    views: t.views?.count == null ? null : Number(t.views.count),
    quoted: quoted ? quoted.replace("https://twitter.com/", "https://x.com/") : null,
    in_reply_to: t.legacy.in_reply_to_status_id_str ?? null,
  };
}

// Pull the posts + the bottom cursor out of a SearchTimeline response; null when the body holds
// no search result at all. Promoted posts are left out.
export function rowsOf(body) {
  const timeline = body?.data?.search_by_raw_query?.search_timeline?.timeline;
  if (!timeline) return null;
  const rows = [];
  let cursor = null;
  for (const ins of timeline.instructions ?? []) {
    const entries = [...(ins.entries ?? []), ...(ins.entry ? [ins.entry] : [])];
    for (const entry of entries) {
      const eid = entry.entryId ?? "";
      if (eid.startsWith("cursor-bottom-")) {
        cursor = entry.content?.value ?? null;
        continue;
      }
      if (eid.startsWith("promoted-")) continue;
      const contents = [entry.content?.itemContent, ...(entry.content?.items ?? []).map((i) => i.item?.itemContent)];
      for (const content of contents) {
        const result = content?.tweet_results?.result;
        const t = result?.tweet ?? result;
        if (t?.legacy) rows.push(toRow(t));
      }
    }
  }
  return { rows, cursor };
}

// Open the shared state, creating it if absent, and make sure every account of the pool has a row.
export function openState(path, usernames) {
  mkdirSync(dirname(path), { recursive: true });
  const db = new DatabaseSync(path);
  // Wait out another process's write instead of throwing SQLITE_BUSY.
  db.exec("PRAGMA busy_timeout=10000");
  db.exec("PRAGMA journal_mode=WAL");
  db.exec(
    "CREATE TABLE IF NOT EXISTS accounts (" +
      "username TEXT PRIMARY KEY NOT NULL COLLATE NOCASE, " +
      "next_at INTEGER NOT NULL DEFAULT 0, " + // ms: the earliest start of this account's next request
      "paused_until INTEGER NOT NULL DEFAULT 0, " + // ms: not picked before this
      "bad TEXT); " + // the 401/403 behind the current pause, NULL for a 429
      "CREATE TABLE IF NOT EXISTS requests (at INTEGER NOT NULL, username TEXT NOT NULL, status TEXT NOT NULL, posts INTEGER NOT NULL)",
  );
  const add = db.prepare("INSERT OR IGNORE INTO accounts (username) VALUES (?)");
  db.exec("BEGIN IMMEDIATE");
  for (const username of usernames) add.run(username);
  db.exec("COMMIT");
  return db;
}

// Reserve the next request: the account of `pool` that is not paused and was used longest ago.
// Returns { username, at } — the request may start at `at` (ms), which keeps GAP_MS after that
// account's previous one — or { resetAt } when every account is paused. One write transaction,
// so two processes never take the same turn.
export function claim(db, pool, now) {
  const inPool = new Set(pool);
  db.exec("BEGIN IMMEDIATE");
  try {
    const rows = db
      .prepare("SELECT username, next_at, paused_until FROM accounts ORDER BY next_at, random()")
      .all()
      .filter((r) => inPool.has(r.username));
    const free = rows.find((r) => r.paused_until <= now);
    if (!free) return { resetAt: Math.min(...rows.map((r) => r.paused_until)) };
    const at = Math.max(now, free.next_at);
    db.prepare("UPDATE accounts SET next_at = ? WHERE username = ?").run(at + GAP_MS, free.username);
    return { username: free.username, at };
  } finally {
    db.exec("COMMIT");
  }
}

// Keep an account out of the rotation until `until` (ms); `bad` names the 401/403 that caused it.
export function pause(db, username, until, bad = null) {
  db.prepare("UPDATE accounts SET paused_until = ?, bad = ? WHERE username = ?").run(until, bad, username);
}

const usable = (db, pool, now) =>
  db
    .prepare("SELECT username FROM accounts WHERE paused_until <= ?")
    .all(now)
    .filter((r) => pool.includes(r.username)).length;

// One SearchTimeline request on one account, cut off after REQUEST_TIMEOUT_MS.
async function request(fetchAs, url, acct) {
  let timer;
  const timeout = new Promise((_, reject) => {
    timer = setTimeout(() => reject(Object.assign(new Error("timeout"), { code: "timeout" })), REQUEST_TIMEOUT_MS);
  });
  try {
    return await Promise.race([
      (async () => {
        const res = await fetchAs(url, acct);
        return { status: res.status, reset: Number(res.headers.get("x-rate-limit-reset")), text: await res.text() };
      })(),
      timeout,
    ]);
  } finally {
    clearTimeout(timer);
  }
}

// The pool, from the client's loadAccounts. It opens the store without a busy timeout and reports
// a locked store (two runs opening it at the same instant) as "no accounts", so ask a few times
// before believing that.
async function loadPool(loadAccounts) {
  for (let attempt = 1; ; attempt++) {
    try {
      return loadAccounts();
    } catch (e) {
      if (attempt === STORE_TRIES) throw e;
      await sleep(100 + Math.random() * 400);
    }
  }
}

// Fill `found` (id -> row) with up to `limit` posts, page by page, each page on the account whose
// turn it is. Throws with the one-line reason when the search cannot be finished.
async function search(found, { query, limit, product }) {
  // The client reads its config when it loads, so the env file comes first. Values already in
  // the environment (exported variables) win.
  loadEnvFile();
  requireEnv("INTEL_X_BEARER_TOKEN");
  requireEnv("INTEL_X_SEARCH_QUERY_ID");
  const { loadAccounts, fetchAs, searchUrl } = await import("../../fetch-x-mentions/scripts/fetch-x-mentions.mjs");
  const accounts = new Map((await loadPool(loadAccounts)).map((a) => [a.label, a]));
  const pool = [...accounts.keys()];
  const db = openState(STATE_PATH, pool);
  const free = usable(db, pool, Date.now());
  if (free < SMALL_POOL) process.stderr.write(`fetch-x-posts: ${free} of ${pool.length} accounts usable\n`);
  const record = db.prepare("INSERT INTO requests (at, username, status, posts) VALUES (?, ?, ?, ?)");

  // searchUrl asks for the Latest product; --top swaps it in the built URL.
  const pageUrl = (cursor) => {
    const url = new URL(searchUrl(query, cursor));
    const variables = { ...JSON.parse(url.searchParams.get("variables")), product };
    url.searchParams.set("variables", JSON.stringify(variables));
    return url.toString();
  };

  let cursor = null;
  let empty = 0;
  let retries = 0;
  let waited = 0;
  const retry = (why) => {
    if (++retries >= MAX_RETRIES) throw new Error(`SearchTimeline failed after ${MAX_RETRIES} attempts (last: ${why})`);
  };
  while (found.size < limit) {
    const turn = claim(db, pool, Date.now());
    const waitMs = (turn.at ?? turn.resetAt) - Date.now();
    if (waitMs > 0) {
      if (waited + waitMs > MAX_WAIT_MS) {
        throw new Error(
          turn.username
            ? `no account is free before ${clock(turn.at)}`
            : `all ${pool.length} accounts are paused; the earliest resets at ${clock(turn.resetAt)}`,
        );
      }
      waited += waitMs;
      await sleep(waitMs);
    }
    if (!turn.username) continue;

    const acct = accounts.get(turn.username);
    const at = Date.now();
    let res;
    try {
      res = await request(fetchAs, pageUrl(cursor), acct);
    } catch (e) {
      // The code only: an undici message can carry the proxy address.
      const why = e.code ?? e.cause?.code ?? e.name ?? "fetch error";
      record.run(at, acct.label, why, 0);
      retry(why);
      continue;
    }
    const { status, reset, text } = res;

    if (status === 429) {
      record.run(at, acct.label, "429", 0);
      pause(db, acct.label, reset ? reset * 1000 : Date.now() + RATE_LIMIT_PAUSE_MS);
      continue;
    }
    // An HTML body on a 4xx is an edge/proxy block, not X's API: the request never reached X.
    const isEdgeBlock = /^\s*<(?:!doctype|html)/i.test(text);
    if ((status === 401 || status === 403) && !isEdgeBlock) {
      record.run(at, acct.label, String(status), 0);
      pause(db, acct.label, Date.now() + BAD_PAUSE_MS, String(status));
      process.stderr.write(`fetch-x-posts: @${acct.label} answered ${status}, marked bad\n`);
      continue;
    }
    if (status !== 200) {
      record.run(at, acct.label, String(status), 0);
      if (status >= 400 && status < 500 && !isEdgeBlock && text.trim() !== "") {
        // A JSON 4xx is a broken request (queryId/features drift), the same on every account.
        throw new Error(`SearchTimeline ${status}: ${text.slice(0, 200).replace(/\s+/g, " ")}`);
      }
      retry(isEdgeBlock ? `${status} edge block (html)` : String(status));
      continue;
    }

    let page = null;
    try {
      page = rowsOf(JSON.parse(text));
    } catch {
      // The proxy can close the stream before the body is complete.
    }
    if (!page) {
      record.run(at, acct.label, "200 bad body", 0);
      retry("200 without a search result");
      continue;
    }
    record.run(at, acct.label, page.rows.length ? "200" : "empty", page.rows.length);
    if (page.rows.length === 0) {
      // X answers a throttled account with an empty page instead of a 429, so one empty page is
      // not the end: ask the next accounts for the same page.
      if (++empty >= EMPTY_ACCOUNTS) return;
      continue;
    }
    empty = 0;
    for (const row of page.rows) if (!found.has(row.id)) found.set(row.id, row);
    if (!page.cursor || page.cursor === cursor) return;
    cursor = page.cursor;
  }
}

async function main() {
  const found = new Map();
  let limit = DEFAULT_LIMIT;
  let reason = null;
  try {
    const args = parseArgs(process.argv.slice(2));
    limit = args.limit;
    await search(found, args);
  } catch (e) {
    reason = e.message;
  }
  const lines = [...found.values()].slice(0, limit).map((row) => JSON.stringify(row) + "\n");
  if (reason) process.stderr.write(`fetch-x-posts: ${reason.split("\n")[0]}\n`);
  // The pool's proxy agents keep the event loop alive, so exit once stdout has flushed.
  process.stdout.write(lines.join(""), () => process.exit(reason ? 1 : 0));
}

if (isMain) await main();
