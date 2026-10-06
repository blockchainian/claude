import { requireEnv, loadEnvFile, stateDir } from "../../fetch-x-mentions/scripts/env.mjs";
// ABOUTME: Fetches one X/Twitter account's own posts and replies over a date range, paging the
// ABOUTME: authenticated SearchTimeline `from:<user>` chronologically, sharded across accounts, resumable per user.
//
// Usage (config from ~/.config/intel/.env loaded automatically):
//   node ${CLAUDE_PLUGIN_ROOT}/skills/fetch-x-user-posts/scripts/fetch-x-user-posts.mjs \
//     <user> [<user> ...] [--file <json|txt>] [--since YYYY-MM-DD] [--until YYYY-MM-DD] [--max-pages <n>]
//   Each <user> is a bare screen name (no @). --file adds more names from a JSON array,
//   a { handles: [...] } / [...] shape, or a newline list. --since defaults to one year
//   before --until; --until defaults to today (exclusive). --max-pages caps each stream at
//   n pages of 20 (default 500 = 10000 posts), a guard against a runaway prolific account.
//
// The auth core (x-client-transaction-id, account list, SearchTimeline request, page parse,
// retry/quota handling, cross-account draining) is reused from the fetch-x-mentions skill,
// the way analyze-x-users reuses analyze-x-mentions's scripts. This script only adds the per-user,
// two-stream (own posts vs replies) timeline logic on top.
//
// Output, per user, under <INTEL_STATE_DIR>/x/kols/<user>/ (lowercased):
//   tweets.jsonl   one own post per line (from:<user> -filter:replies -filter:nativeretweets)
//   replies.jsonl  one reply per line     (from:<user> filter:replies)
//     each line: id, author, author_name, author_followers, author_verified, text, created_at,
//     likes, retweets, replies, quotes, views, lang, url  (deduplicated by id, newest first)
//   posts.out.json progress: { since, until, tweets: {count, done}, replies: {count, done} }
//     A stream is `done` once its full page-through finished for this exact window; a rerun
//     skips done streams and refetches the rest, so an interrupted run resumes by rerunning.
import { realpathSync } from "node:fs";

import { readFile, writeFile, mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { join } from "node:path";

import {
  drainPlan,
  fetchAs,
  generateTransactionId,
  getPage,
  loadAccounts,
  parsePage,
  searchUrl,
  sortTweets,
  untilInstant,
} from "../../fetch-x-mentions/scripts/fetch-x-mentions.mjs";

const isMain = process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) loadEnvFile();

const PAGE = 20; // tweets per SearchTimeline page
const DEFAULT_MAX_PAGES = 500; // 10000 posts per stream (20/page) — a guard, not an expected cap
const DEFAULT_MAX_TRIES = 3; // attempts a still-incomplete stream gets before it is accepted as-is
const REQUEST_SPACING_MS = 800;

// The two timeline streams saved per user, and the search filter that selects each.
export const STREAMS = [
  { name: "tweets", filter: "-filter:replies -filter:nativeretweets" },
  { name: "replies", filter: "filter:replies" },
];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// --- UserByScreenName: does this handle exist, before we spend any fetch on it? ---
// queryId is captured from x.com's authed web bundle (X_USER_QUERY_ID); the feature switches and
// field toggles are the exact set that operation declares there. Re-extract when X answers 404.
const USER_QID = process.env.X_USER_QUERY_ID;
const USER_FEATURES = {
  hidden_profile_subscriptions_enabled: true,
  profile_label_improvements_pcf_label_in_post_enabled: true,
  responsive_web_profile_redirect_enabled: true,
  rweb_tipjar_consumption_enabled: true,
  verified_phone_label_enabled: false,
  subscriptions_verification_info_is_identity_verified_enabled: true,
  subscriptions_verification_info_verified_since_enabled: true,
  highlights_tweets_tab_ui_enabled: true,
  responsive_web_twitter_article_notes_tab_enabled: true,
  subscriptions_feature_can_gift_premium: true,
  creator_subscriptions_tweet_preview_api_enabled: true,
  responsive_web_graphql_timeline_navigation_enabled: true,
};
const USER_FIELD_TOGGLES = { withPayments: false, withAuxiliaryUserLabels: false };

const userPath = () => `/i/api/graphql/${requireEnv("X_USER_QUERY_ID")}/UserByScreenName`;

export function userUrl(name) {
  requireEnv("X_USER_QUERY_ID");
  const params = new URLSearchParams({
    variables: JSON.stringify({ screen_name: name }),
    features: JSON.stringify(USER_FEATURES),
    fieldToggles: JSON.stringify(USER_FIELD_TOGGLES),
  });
  return `https://x.com${userPath()}?${params.toString()}`;
}

function userHeaders(acct) {
  return {
    authorization: `Bearer ${requireEnv("X_BEARER_TOKEN")}`,
    "x-csrf-token": acct.ct0,
    cookie: `auth_token=${acct.authToken}; ct0=${acct.ct0}`,
    "x-twitter-auth-type": "OAuth2Session",
    "x-twitter-active-user": "yes",
    "x-twitter-client-language": "en",
    "x-client-transaction-id": generateTransactionId("GET", userPath()),
    "content-type": "application/json",
    "User-Agent":
      "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 " +
      "(KHTML, like Gecko) Version/17.0 Safari/605.1.15",
    Referer: "https://x.com/",
  };
}

const fetchUserAs = (url, acct) =>
  fetch(url, { dispatcher: acct.dispatcher, headers: userHeaders(acct) });

// Classify a UserByScreenName result into how (and whether) it is worth fetching:
//   missing      — no live User result (renamed / deactivated / UserUnavailable): nothing to fetch.
//   protected    — a live but private account: its posts are never searchable, so a search is 0.
//   never_posted — a live account with 0 lifetime tweets: nothing to fetch.
//   exists       — a live, readable account with posts; carries lifetime_tweets and created_at so
//                  the fetch can tell a real in-window silence from a search-invisible miss.
export function classifyUser(body) {
  const r = body?.data?.user?.result;
  if (!r || r.__typename !== "User" || !r.rest_id) return { state: "missing" };
  if (r.privacy?.protected) return { state: "protected", id: r.rest_id };
  const lifetime = r.tweet_counts?.tweets ?? null;
  const created = r.core?.created_at ? new Date(r.core.created_at).toISOString() : null;
  if (lifetime === 0) return { state: "never_posted", id: r.rest_id, lifetime, created };
  return { state: "exists", id: r.rest_id, lifetime, created };
}

// Resolve one handle via UserByScreenName, reusing getPage's retry/429/edge handling. Returns the
// classifyUser object; a non-200 (never happens — getPage retries transients) is treated as missing.
export async function resolveUser(acct, name, fetchPage = getPage) {
  const { status, body } = await fetchPage(userUrl(name), acct, fetchUserAs);
  if (status !== 200 || !body) return { state: "missing" };
  return classifyUser(body);
}

// --- UserTweetsAndReplies: the account's real timeline, by user id, for handles X search won't
// return (new or search-deboosted accounts). Not search, so it is not subject to search visibility.
const UTAR_QID = process.env.X_TIMELINE_QUERY_ID;
const utarPath = () => `/i/api/graphql/${requireEnv("X_TIMELINE_QUERY_ID")}/UserTweetsAndReplies`;

export function userTimelineUrl(userId, cursor) {
  const variables = { userId, count: 40, includePromotedContent: false, withCommunity: true, withVoice: true };
  if (cursor) variables.cursor = cursor;
  // This operation needs no feature switches (verified against the live endpoint).
  const params = new URLSearchParams({ variables: JSON.stringify(variables), features: JSON.stringify({}) });
  return `https://x.com${utarPath()}?${params.toString()}`;
}

function timelineHeaders(acct) {
  return {
    authorization: `Bearer ${requireEnv("X_BEARER_TOKEN")}`,
    "x-csrf-token": acct.ct0,
    cookie: `auth_token=${acct.authToken}; ct0=${acct.ct0}`,
    "x-twitter-auth-type": "OAuth2Session",
    "x-twitter-active-user": "yes",
    "x-twitter-client-language": "en",
    "x-client-transaction-id": generateTransactionId("GET", utarPath()),
    "content-type": "application/json",
    "User-Agent":
      "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 " +
      "(KHTML, like Gecko) Version/17.0 Safari/605.1.15",
    Referer: "https://x.com/",
  };
}

const fetchTimelineAs = (url, acct) =>
  fetch(url, { dispatcher: acct.dispatcher, headers: timelineHeaders(acct) });

// One tweet's fields out of a timeline `tweet_results.result` — the same shape SearchTimeline uses,
// plus is_reply / is_retweet and the author's id (timeline modules also carry other people's posts).
export function extractTweet(result) {
  const t = result?.tweet ?? result;
  if (!t || !t.legacy) return null;
  const u = t.core?.user_results?.result;
  const ul = u?.legacy ?? {};
  const leg = t.legacy;
  const text = leg.full_text ?? t.note_tweet?.note_tweet_results?.result?.text ?? "";
  return {
    id: t.rest_id ?? leg.id_str,
    author: ul.screen_name ?? u?.core?.screen_name ?? null,
    author_name: ul.name ?? u?.core?.name ?? null,
    author_id: u?.rest_id ?? null,
    author_followers: ul.followers_count ?? null,
    author_verified: u?.is_blue_verified ?? ul.verified ?? null,
    text,
    created_at: leg.created_at ?? null,
    likes: leg.favorite_count ?? 0,
    retweets: leg.retweet_count ?? 0,
    replies: leg.reply_count ?? 0,
    quotes: leg.quote_count ?? 0,
    views: t.views?.count ?? null,
    lang: leg.lang ?? null,
    url: `https://x.com/${ul.screen_name ?? "i"}/status/${t.rest_id ?? leg.id_str}`,
    is_reply: !!leg.in_reply_to_status_id_str,
    is_retweet: !!leg.retweeted_status_result || /^RT @/.test(text),
    sources: [],
  };
}

// Pull the account's own posts + the bottom cursor out of a UserTweetsAndReplies timeline. Walks
// direct items, the pinned entry, and conversation modules (where replies live); keeps only posts
// authored by `userId` (modules also carry the tweets they reply to) and drops retweets.
export function parseUserTimeline(body, userId) {
  const out = [];
  let cursor = null;
  const uid = String(userId);
  const take = (result) => {
    const tw = extractTweet(result);
    if (tw && !tw.is_retweet && String(tw.author_id) === uid) out.push(tw);
  };
  const instructions = body?.data?.user?.result?.timeline?.timeline?.instructions ?? [];
  for (const insn of instructions) {
    if (insn.type === "TimelinePinEntry" && insn.entry) {
      take(insn.entry.content?.itemContent?.tweet_results?.result);
    }
    for (const e of insn.entries ?? []) {
      const eid = e.entryId ?? "";
      const c = e.content;
      if (eid.startsWith("cursor-bottom")) {
        cursor = c?.value ?? null;
        continue;
      }
      if (c?.itemContent?.tweet_results?.result) take(c.itemContent.tweet_results.result);
      for (const it of c?.items ?? []) take(it?.item?.itemContent?.tweet_results?.result);
    }
  }
  return { posts: out, cursor };
}

// Page an account's timeline newest→oldest into `out`, keeping posts on/after `since`. Complete
// when a page reaches past the since floor, or the timeline ends (no cursor / empty) — only the
// maxPages cap leaves it incomplete. Throws (via getPage) on a paused-account failure.
export async function fetchTimeline(
  acct,
  userId,
  since,
  maxPages,
  out = [],
  fetchPage = getPage,
  wait = sleep,
) {
  const seen = new Set(out.map((t) => t.id));
  const sinceMs = Date.parse(`${since}T00:00:00Z`);
  let cursor = null;
  for (let page = 0; page < maxPages; page++) {
    const { status, body } = await fetchPage(userTimelineUrl(userId, cursor), acct, fetchTimelineAs);
    if (status !== 200 || !body) return false;
    const { posts, cursor: next } = parseUserTimeline(body, userId);
    let oldestMs = Infinity;
    for (const tw of posts) {
      const ms = Date.parse(tw.created_at);
      if (ms < oldestMs) oldestMs = ms;
      if (ms >= sinceMs && !seen.has(tw.id)) {
        seen.add(tw.id);
        out.push(tw);
      }
    }
    if (posts.length && oldestMs < sinceMs) return true; // paged past the window start — complete
    if (posts.length === 0 || !next || next === cursor) return true; // end of the timeline — got it all
    cursor = next;
    await wait(REQUEST_SPACING_MS);
  }
  return false; // hit maxPages — likely more
}

// A screen name stripped of a leading @, an x.com/twitter.com URL, or a trailing path, lowercased.
export function normHandle(raw) {
  if (!raw) return null;
  let s = String(raw).trim();
  s = s.replace(/^https?:\/\/(www\.)?(x|twitter)\.com\//i, "");
  s = s.replace(/^@/, "").replace(/[/?#].*$/, "").trim();
  return /^[a-z0-9_]{1,15}$/i.test(s) ? s.toLowerCase() : null;
}

// The day one year before `until`, the default start of the window.
export function oneYearBefore(until) {
  const d = new Date(until + "T00:00:00Z");
  d.setUTCFullYear(d.getUTCFullYear() - 1);
  return d.toISOString().slice(0, 10);
}

// The SearchTimeline query for one user's stream over [since, untilInstant), pinned to UTC
// instants (see fetch-x-mentions dayQuery for why bare since:/until: dates drift). `since` is a
// day; `untilInstant` is a full instant string, exclusive — the window end on a first attempt,
// or one second past the oldest post already held so a retry continues backwards from there.
export function streamQuery(user, filter, since, untilInstant) {
  return `from:${user} ${filter} since:${since}_00:00:00_UTC until:${untilInstant}`;
}

// The window end for an attempt: continue just past the oldest post already held, else the run's
// window end. Mirrors fetch-x-mentions' refill (untilInstant brings the boundary post back, a
// duplicate the dedup drops, so nothing between is skipped).
export function attemptUntil(oldest, until) {
  return oldest ? untilInstant(oldest) : `${until}_00:00:00_UTC`;
}

// Split argv into positional user names, --file paths, the date window and the page cap.
export function parseArgs(argv) {
  const positional = [];
  const files = [];
  let since = null;
  let until = null;
  let maxPages = DEFAULT_MAX_PAGES;
  let maxTries = DEFAULT_MAX_TRIES;
  const value = (a, i) => (a.includes("=") ? a.slice(a.indexOf("=") + 1) : argv[++i.v]);
  const posInt = (raw, opt) => {
    const n = Number(raw);
    if (!Number.isInteger(n) || n < 1) throw new Error(`${opt} needs a positive integer`);
    return n;
  };
  for (const it = { v: 0 }; it.v < argv.length; it.v++) {
    const a = argv[it.v];
    if (a === "--file" || a.startsWith("--file=")) files.push(value(a, it));
    else if (a === "--since" || a.startsWith("--since=")) since = value(a, it);
    else if (a === "--until" || a.startsWith("--until=")) until = value(a, it);
    else if (a === "--max-pages" || a.startsWith("--max-pages=")) maxPages = posInt(value(a, it), "--max-pages");
    else if (a === "--max-tries" || a.startsWith("--max-tries=")) maxTries = posInt(value(a, it), "--max-tries");
    else if (a.startsWith("--")) throw new Error(`Unknown option ${a}`);
    else positional.push(a);
  }
  return { positional, files, since, until, maxPages, maxTries };
}

// Pull screen names out of a --file: a JSON array, a { handles|users|kols: [...] } object
// whose entries are strings or { x }-shaped rows, or a plain newline list.
export function handlesFromFile(text) {
  let data;
  try {
    data = JSON.parse(text);
  } catch {
    return text.split("\n");
  }
  const arr = Array.isArray(data)
    ? data
    : data && typeof data === "object" && Object.values(data).every(Array.isArray)
      ? Object.values(data).flat()
      : (() => { throw new Error("roster JSON must be an array or an object of username arrays"); })();
  return (Array.isArray(arr) ? arr : []).map((e) =>
    typeof e === "string" ? e : (e?.x ?? e?.handle ?? e?.username ?? ""),
  );
}

// The unique, valid, order-preserved handles from positional args and every --file.
export async function collectHandles(positional, files) {
  const raw = [...positional];
  for (const f of files) raw.push(...handlesFromFile(await readFile(f, "utf8")));
  const seen = new Set();
  const out = [];
  for (const r of raw) {
    const h = normHandle(r);
    if (h && !seen.has(h)) {
      seen.add(h);
      out.push(h);
    }
  }
  return out;
}

async function loadJson(path, fallback) {
  try {
    return JSON.parse(await readFile(path, "utf8"));
  } catch {
    return fallback;
  }
}

// The posts already saved for a stream, or [] if the file is missing. A resume merges its batch
// with these so earlier pages are kept.
async function loadJsonl(path) {
  try {
    return (await readFile(path, "utf8")).split("\n").filter(Boolean).map((l) => JSON.parse(l));
  } catch {
    return [];
  }
}

// The saved progress reused only for the same window, else a fresh one. Per stream: `count`,
// `complete` (a genuine tail was reached), and `tries` (attempts spent). A different window
// resets everything so it refetches.
const freshStream = () => ({ count: 0, complete: false, tries: 0, oldest: null });
export function openProgress(saved, since, until) {
  if (!saved || saved.since !== since || saved.until !== until) {
    return { since, until, exists: null, profile: null, timeline_done: false, tweets: freshStream(), replies: freshStream() };
  }
  const keep = (s) => ({
    count: s?.count ?? 0,
    complete: !!(s?.complete ?? s?.done), // tolerate the earlier {count, done} shape
    tries: s?.tries ?? (s?.complete || s?.done ? 1 : 0),
    oldest: s?.oldest ?? null, // where the last attempt stopped; a retry continues backwards from here
  });
  // exists: whether UserByScreenName resolved the handle to a live account (null = not resolved).
  // profile: the resolved signals ({state, id, lifetime, created}) — kept so the fetch can tell a
  // real in-window silence from a search-invisible miss, and so a rerun need not re-resolve.
  return {
    since,
    until,
    exists: saved.exists ?? null,
    profile: saved.profile ?? null,
    timeline_done: saved.timeline_done ?? false,
    tweets: keep(saved.tweets),
    replies: keep(saved.replies),
  };
}

// A stream is settled — not worth another attempt — once it reached a genuine tail, or it has
// used up its attempts (an empty/boundary stop that never resolved is accepted as-is then).
export function isSettled(stream, maxTries) {
  return !!stream && (stream.complete || (stream.tries ?? 0) >= maxTries);
}

// The users to resolve first: existence unknown and not already fully fetched (a user whose two
// streams are complete plainly exists, so it needs no lookup). Resolving before fetching means a
// dead handle costs one profile request, not a full windowed fetch with retries.
export function planResolve(users, progressByUser) {
  return users.filter((user) => {
    const p = progressByUser[user];
    const bothComplete = p?.tweets?.complete && p?.replies?.complete;
    // Resolve any handle without a profile yet that is not already fully fetched — a fully-fetched
    // handle plainly exists and has data, so it needs no lookup; an empty or partial one does, so
    // its existence and post count can steer the search retries and the timeline fallback.
    return !p?.profile && !bothComplete;
  });
}

// The (user, stream) units this run fetches: every stream not yet settled, skipping handles known
// not to exist (nothing to fetch there). Order keeps a user's two streams adjacent, so accounts
// spread across users.
export function planWork(users, progressByUser, maxTries) {
  const work = [];
  for (const user of users) {
    const p = progressByUser[user];
    if (p?.exists === false) continue; // dead handle — resolution already settled its streams
    for (const s of STREAMS) {
      if (!isSettled(p?.[s.name], maxTries)) {
        work.push({ user, stream: s.name, filter: s.filter, day: `${user}/${s.name}` });
      }
    }
  }
  return work;
}

// One stream of one user: page from newest through the window, appending new posts into `out`
// (passed in so a throw preserves what was already fetched). Throws (via getPage) on a
// paused-account failure so the unit is requeued and resumed from its new oldest. Returns how it
// stopped, which the caller turns into complete/gap given how much the stream already holds:
//   "complete" — a short page (0 < len < PAGE): the reliable, unambiguous tail.
//   "end"      — an empty page / no cursor: no more results are available. With data already held
//                this means the floor was reached (done); with none held it is ambiguous (a
//                search-invisible account or a throttle) and left a gap for the timeline fallback.
//   "capped"   — hit maxPages, or a bad response: there is likely more, so retry.
export async function fetchStream(
  acct,
  query,
  maxPages,
  out = [],
  fetchPage = getPage,
  wait = sleep,
) {
  const seen = new Set(out.map((t) => t.id));
  let cursor = null;
  for (let page = 0; page < maxPages; page++) {
    const { status, body } = await fetchPage(searchUrl(query, cursor), acct, fetchAs);
    if (status !== 200 || !body) return "capped";
    const { tweets, cursor: next } = parsePage(body, []);
    for (const tw of tweets) {
      if (!tw.id || seen.has(tw.id)) continue;
      seen.add(tw.id);
      out.push(tw);
    }
    if (tweets.length > 0 && tweets.length < PAGE) return "complete"; // short page = genuine tail
    if (tweets.length === 0 || !next || next === cursor) return "end"; // no more results available
    cursor = next;
    await wait(REQUEST_SPACING_MS);
  }
  return "capped"; // hit maxPages — likely more
}

// Turn a fetchStream stop reason into whether the stream is complete, given whether the account is
// search-reachable (`reachable` = it holds any posts, judged at the user level by the caller). A
// short-page tail is always complete; reaching the end of results is complete when the account is
// reachable (so an empty stream is genuinely empty — e.g. a reply-only account's originals); an
// account with no posts at all that ends is the ambiguous search-invisible case, left a gap for the
// timeline fallback. Hitting the page cap is never complete.
export function streamComplete(status, reachable) {
  if (status === "complete") return true;
  if (status === "end") return reachable;
  return false;
}

async function main() {
  const { positional, files, since: sinceArg, until: untilArg, maxPages, maxTries } =
    parseArgs(process.argv.slice(2));
  const until = untilArg || new Date().toISOString().slice(0, 10);
  const since = sinceArg || oneYearBefore(until);
  const users = await collectHandles(positional, files);
  if (!users.length) {
    console.error(
      'Usage: node ' +
        "${CLAUDE_PLUGIN_ROOT}/skills/fetch-x-user-posts/scripts/fetch-x-user-posts.mjs <user> [<user> ...] " +
        "[--file <path>] [--since D] [--until D] [--max-pages n]",
    );
    process.exit(1);
  }

  requireEnv("X_BEARER_TOKEN");
  requireEnv("X_SEARCH_QUERY_ID");
  const accounts = loadAccounts();
  const dirOf = (user) => join(stateDir(), "x", "kols", user);
  const progressByUser = {};
  for (const user of users) {
    progressByUser[user] = openProgress(
      await loadJson(join(dirOf(user), "posts.out.json"), null),
      since,
      until,
    );
  }

  const writeProgress = async (user) => {
    const dir = dirOf(user);
    await mkdir(dir, { recursive: true });
    await writeFile(join(dir, "posts.out.json"), JSON.stringify(progressByUser[user], null, 1));
  };

  // Resolve existence first (when X_USER_QUERY_ID is set): a dead / renamed / suspended handle
  // costs one UserByScreenName lookup here instead of a full windowed fetch with retries. A live
  // handle is fetched normally; a missing one has both streams settled to zero and is skipped.
  if (USER_QID) {
    const toResolve = planResolve(users, progressByUser);
    if (toResolve.length) {
      process.stderr.write(
        `fetch-x-user-posts: resolving ${toResolve.length} handle(s) via UserByScreenName\n`,
      );
      const tally = { missing: 0, protected: 0, never_posted: 0, exists: 0 };
      await drainPlan(
        accounts,
        toResolve.map((user) => ({ user, day: `resolve/${user}` })),
        async (acct, { user }) => {
          const res = await resolveUser(acct, user);
          const p = progressByUser[user];
          p.exists = res.state !== "missing";
          p.profile = { state: res.state, id: res.id ?? null, lifetime: res.lifetime ?? null, created: res.created ?? null };
          // Nothing to fetch via search when the account is gone, private, or has never posted —
          // settle both streams to zero so no search or retry is spent on them.
          if (res.state !== "exists") {
            for (const s of STREAMS) {
              p[s.name] = { count: 0, complete: true, tries: p[s.name]?.tries ?? 0, oldest: null };
            }
          }
          tally[res.state] = (tally[res.state] ?? 0) + 1;
          await writeProgress(user);
          process.stderr.write(`  [${acct.label}] resolve ${user}: ${res.state}\n`);
        },
        sleep,
        (line) => process.stderr.write(line + "\n"),
      );
      process.stderr.write(
        `fetch-x-user-posts: resolved — ${tally.exists} live, ${tally.missing} gone, ` +
          `${tally.protected} private, ${tally.never_posted} never-posted (last three skipped)\n`,
      );
    }
  }

  const work = planWork(users, progressByUser, maxTries);
  process.stderr.write(
    `fetch-x-user-posts: ${users.length} users, ${work.length} streams to fetch ` +
      `across ${accounts.length} account(s), window ${since}..${until}\n`,
  );

  // Each attempt commits through one chain: merge the batch into the on-disk file (dedup, newest
  // first), then update and persist progress, so two accounts committing different users never
  // tear a file. `posts` are only the batch fetched this attempt; they are merged with what the
  // file already holds so a resumed stream keeps its earlier pages. `oldest` (ISO of the oldest
  // post held) is where the next attempt continues backwards from. `countTry` is false when the
  // attempt threw mid-way: the partial is saved but the try is not spent, so the resume is free.
  let checkpoint = Promise.resolve();
  const commit = (user, stream, posts, complete, countTry) => {
    checkpoint = checkpoint.then(async () => {
      const dir = dirOf(user);
      await mkdir(dir, { recursive: true });
      const file = join(dir, `${stream}.jsonl`);
      const held = await loadJsonl(file);
      const sorted = sortTweets([...held, ...posts]); // newest first, deduped by id
      await writeFile(
        file,
        sorted.length ? sorted.map((t) => JSON.stringify(t)).join("\n") + "\n" : "",
      );
      const p = progressByUser[user];
      const prev = p[stream] ?? {};
      p[stream] = {
        count: sorted.length,
        complete,
        tries: (prev.tries ?? 0) + (countTry ? 1 : 0),
        oldest: sorted.length ? new Date(sorted.at(-1).created_at).toISOString() : (prev.oldest ?? null),
      };
      await writeFile(join(dir, "posts.out.json"), JSON.stringify(p, null, 1));
    });
    return checkpoint;
  };

  const { paused, left } = await drainPlan(
    accounts,
    work,
    async (acct, { user, stream, filter }) => {
      const prev = progressByUser[user][stream] ?? {};
      const oldest = prev.oldest ?? null;
      const query = streamQuery(user, filter, since, attemptUntil(oldest, until));
      const batch = [];
      try {
        const status = await fetchStream(acct, query, maxPages, batch);
        // Complete if the tail was reached, or the results ended and the account is search-reachable.
        // Reachability is judged at the USER level: if this batch, this stream, or the OTHER stream
        // holds posts, search sees this account, so an empty stream is genuinely empty (e.g. a
        // reply-only account has 0 originals) → done. Only a user with zero posts in both streams is
        // the ambiguous search-invisible case, left a gap for the timeline fallback.
        const up = progressByUser[user];
        const userHasPosts =
          batch.length > 0 || (up.tweets?.count ?? 0) > 0 || (up.replies?.count ?? 0) > 0;
        const complete = streamComplete(status, userHasPosts);
        await commit(user, stream, batch, complete, true);
        process.stderr.write(
          `  [${acct.label}] ${user}/${stream}: +${batch.length} (${status}${complete ? ", done" : ", gap"})\n`,
        );
      } catch (e) {
        // Persist what this attempt fetched before the account was paused, without spending a
        // try, so the requeued unit resumes from the new oldest instead of re-paging from now.
        await commit(user, stream, batch, false, false);
        throw e;
      }
    },
    sleep,
    (line) => process.stderr.write(line + "\n"),
  );
  if (paused.length) {
    process.stderr.write(
      `fetch-x-user-posts: paused ${paused.join(", ")}; ${left} stream(s) left\n`,
    );
  }

  // Timeline fallback: X search does not return some live accounts (new or search-deboosted). For a
  // live handle whose profile shows posts but whose search came back empty, fetch its real timeline
  // directly (UserTweetsAndReplies by id) — authoritative, not search-gated — and split it into the
  // two files. A timeline that is also empty in-window confirms genuine inactivity.
  if (UTAR_QID) {
    const invisible = users.filter((u) => {
      const p = progressByUser[u];
      const both0 = (p.tweets?.count || 0) + (p.replies?.count || 0) === 0;
      return (
        p.profile?.state === "exists" &&
        p.profile.id &&
        (p.profile.lifetime ?? 0) > 0 &&
        both0 &&
        !p.timeline_done
      );
    });
    if (invisible.length) {
      process.stderr.write(
        `fetch-x-user-posts: ${invisible.length} live handle(s) empty in search — recovering via timeline\n`,
      );
      await drainPlan(
        accounts,
        invisible.map((user) => ({ user, day: `timeline/${user}` })),
        async (acct, { user }) => {
          const p = progressByUser[user];
          const batch = [];
          try {
            const done = await fetchTimeline(acct, p.profile.id, since, maxPages, batch);
            const tw = batch.filter((t) => !t.is_reply);
            const rp = batch.filter((t) => t.is_reply);
            p.timeline_done = done; // set before commit so the persisted progress carries it
            await commit(user, "tweets", tw, done, true);
            await commit(user, "replies", rp, done, true);
            process.stderr.write(
              `  [${acct.label}] timeline ${user}: +${tw.length}t/${rp.length}r${done ? " (done)" : " (partial)"}\n`,
            );
          } catch (e) {
            await commit(user, "tweets", batch.filter((t) => !t.is_reply), false, false);
            await commit(user, "replies", batch.filter((t) => t.is_reply), false, false);
            throw e;
          }
        },
        sleep,
        (line) => process.stderr.write(line + "\n"),
      );
    }
  }

  const complete = users.filter(
    (u) => progressByUser[u].tweets.complete && progressByUser[u].replies.complete,
  ).length;
  const unsettled = planWork(users, progressByUser, maxTries).length;
  if (unsettled) process.exitCode = 1; // streams still worth another attempt; rerun to continue
  console.log(
    `fetch-x-user-posts: ${complete}/${users.length} users fully complete, ${unsettled} stream(s) ` +
      `still worth a retry -> <INTEL_STATE_DIR>/x/kols/ (rerun until it exits 0)`,
  );
}

if (isMain) await main();
