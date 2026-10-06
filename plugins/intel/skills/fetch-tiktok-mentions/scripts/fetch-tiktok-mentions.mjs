import { requireEnv, loadEnvFile } from "../../fetch-x-mentions/scripts/env.mjs";
// ABOUTME: Fetches a brand's TikTok videos from hashtag pages, user pages and keyword searches, each video's
// ABOUTME: comments and its video file, through Camoufox sessions on the ISP proxy pool, resumable.
//
// Usage (config from ~/.config/intel/.env loaded automatically):
//   node ${CLAUDE_PLUGIN_ROOT}/skills/fetch-tiktok-mentions/scripts/fetch-tiktok-mentions.mjs \
//     <slug> [--hashtag <name>]... [--user <handle>]... [--keyword <words>]... \
//     [--hashtag-min-plays <n>] \
//     [--source-limit <n>] [--comment-limit <n>] [--sessions <n>] [--rate <n>] [--concurrency <n>] [--out <dir>] [--no-comments] [--no-download]
//   <slug> names the output dir docs/intel/tiktok/<slug>/ (from the repo root); --out names another one. The sources are remembered in
//   videos.out.json, so a rerun needs only the slug; sources given again are added to the saved ones.
//   --hashtag-min-plays drops a hashtag page's videos below that many plays (default 10000); a
//   user's videos and a keyword's results have no floor. Videos with a non-English caption are
//   dropped from every source.
//   --keyword is TikTok's video search for those words: it also finds videos that mention the brand
//   without its hashtag. It needs a logged-in account (below) and gives a few hundred results.
//   A hashtag page shows a different sample of its videos each time it is asked, so up to four
//   sessions pull it once each, at the same time, and the pulls are merged.
//   --source-limit caps the videos one pull of a source may yield (default 1000).
//   --comment-limit caps the comments kept per video, top-level and replies together (default 1000):
//   every top-level comment first, then replies of the most liked and replied threads.
//   --sessions is how many browser sessions work at once, one per ISP slot (default: every slot);
//   --rate is how many requests one session starts per second (default 20), --concurrency how
//   many it may have in flight (default 12, enough to reach the rate at ~0.5 s per request).
//
// Invented example:
//   node ${CLAUDE_PLUGIN_ROOT}/skills/fetch-tiktok-mentions/scripts/fetch-tiktok-mentions.mjs \
//     demofun --hashtag demofun --hashtag demodotfun --user demo.fun
//
// How a request is made. TikTok's web API rejects a request that is not signed by its own page
// script (an empty 200). So each session opens tiktok.com in Camoufox, copies the query params of
// the first API request the page itself sends (device id, region, screen ...), and then calls
// fetch() inside the page's main world: TikTok's script wraps window.fetch there and signs the
// request. A session is anonymous and bound to one ISP slot (a fixed IP). Each session paces requests to the configured rate
// to limit bursts and throttling.
// A refusal ("Access Denied", a non-JSON body) is held against the session, not the IP, and stays
// for minutes: the session is closed and a fresh one on the same slot takes the work over at once.
// An empty body is the IP being refused for a while: the slot's next session opens after a cool-down.
// Video files are fetched inside the session too (the play address is signed for the session that
// asked for it).
//
// The account. TikTok answers a keyword search only to a logged-in viewer, and gives an anonymous
// one a single page of a user's timeline. Both go through the account the secrets-manager skill
// logged in (`login tiktok`): the first active row of its `tiktok` table, opened as the same
// browser profile, on the ISP slot it logged in from, paced well below an anonymous session.
// Without an account a keyword is not collected and a user's timeline stays one page.
//
// Config:
//   ISP_PROXY_URL    the ISP pool's base url; slot n of the pool is the base port + n, one fixed IP each.
//   ISP_PROXY_COUNT  how many slots the pool has (default 1).
//   SECRETS_MANAGER_STATE_PATH  where the secrets-manager store and profiles are (default ~/.config/secrets-manager).
//
// Output under docs/intel/tiktok/<slug>/, or the --out directory:
//   videos.jsonl          one video per line, TikTok's full item plus a `sources` array of the
//                         hashtag / user pages and keyword searches that surfaced it; deduplicated
//                         by id, newest first.
//   comments/<id>.jsonl   one comment per line, TikTok's full comment; top-level comments most
//                         popular first, each followed by its replies (a reply's `reply_id` is
//                         its parent's `cid`).
//   videos.out.json       the progress file and the only input to planning:
//     { sources: { hashtags, users, keywords }, commentLimit,
//       runs: [{ at, fetched, kept, new, sources: { "<label>": { fetched, kept, new, complete, pulls } } }],
//       videos: { "<id>": { comments: { count, complete }, downloaded } } }
//     Per run and source: `fetched` videos TikTok returned, `kept` those at or above the play
//     floor and in English, `new` those kept and not held before, `pulls` how often the source was paged.
// Video files are too big for git, so they live outside the repo, shared by every slug:
//   ~/.local/share/tiktok/<id>.mp4. TIKTOK_VIDEOS_DIR overrides the directory.
//   A run collects every source, then fetches comments for the videos whose comments are not
//   complete and downloads the videos without a file. Rerun the same command to resume.

import { mkdir, readFile, writeFile } from "node:fs/promises";
import { existsSync, readFileSync, realpathSync } from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { fileURLToPath } from "node:url";

const isMain = process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) loadEnvFile();

const START_URL = "https://www.tiktok.com/explore";
const API = "https://www.tiktok.com/api/";
// The signatures TikTok's page script adds to a request; a new request gets its own.
const SIGN_KEYS = ["X-Bogus", "X-Gnarly", "X-Dynosaur", "msToken"];
const COMMENT_PAGE = 20;
const DEFAULT_SOURCE_LIMIT = 1000;
const DEFAULT_COMMENT_LIMIT = 1000;
const DEFAULT_RATE = 20; // default requests per second one session starts
const DEFAULT_CONCURRENCY = 12; // requests one session may have in flight (~0.5 s each, so 12 reach 20/s)
// A hashtag page mixes popular videos with fresh ones nobody watched, hence its floor. A search is
// ranked by relevance and ends after a few hundred results, so all of them are kept.
const DEFAULT_MIN_PLAYS = { hashtag: 10000, user: 0, keyword: 0 };
const ACCOUNT_RATE = 2; // requests per second through the logged-in account: a person's pace, it has no need for more
const SESSION_COOKIE = "sessionid"; // the cookie of a signed-in tiktok.com browser
const SESSION_COOLDOWN_MS = 60000; // how long a slot rests after TikTok answered its IP with empty bodies
const REQUEST_TIMEOUT_MS = 30000;
const DOWNLOAD_TIMEOUT_MS = 600000; // a file fetch shares the slot's bandwidth with the others in flight
const DOWNLOAD_LANES = 4; // file fetches in flight per session: bandwidth-bound, more only time out
const MAX_DOWNLOAD_BYTES = 200e6; // a file is carried out of the page as one base64 string
const EMPTY_RETRY_MS = 3000;
const TEMPLATE_WAIT_MS = 30000;
const MAX_LANE_FAILURES = 3; // failures in a row (blocked sessions, sessions that won't open) before a lane stops
const KEPT_LANGUAGES = ["en", "un"]; // caption languages kept: English, and "un" (TikTok could not tell)
const HASHTAG_PULLS = 4; // sessions that pull one hashtag: measured 42, 52, 56, 58 kept videos, then no more
const STALE_PAGES = 2; // pages in a row with nothing unseen before a source is taken as repeating

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// TikTok refused a request. `scope` says what it holds against: "session" (the browser session;
// a fresh one on the same IP is fine) or "ip" (the slot needs a cool-down).
export class Blocked extends Error {
  constructor(message, scope = "session") {
    super(message);
    this.scope = scope;
  }
}

// TikTok refused the logged-in account a request it only answers to a signed-in viewer.
export class AccountRefused extends Error {}

// One place videos come from: { kind, value, label }. A name may be given bare, as #tag / @user,
// or as its tiktok.com url; a keyword is the words searched for. Returns null for an unusable
// value or an unknown kind.
export function sourceOf(kind, raw) {
  const s = String(raw ?? "").trim();
  if (kind === "hashtag") {
    const value = s.replace(/^https?:\/\/(www\.)?tiktok\.com\/tag\//i, "").replace(/[/?].*$/, "").replace(/^#/, "").toLowerCase();
    return value && !/\s/.test(value) ? { kind, value, label: `#${value}` } : null;
  }
  if (kind === "user") {
    const value = s.replace(/^https?:\/\/(www\.)?tiktok\.com\//i, "").replace(/^@/, "").replace(/[/?].*$/, "").toLowerCase();
    return /^[a-z0-9._]{1,24}$/.test(value) ? { kind, value, label: `@${value}` } : null;
  }
  if (kind === "keyword") {
    const value = s.replace(/\s+/g, " ").toLowerCase();
    return value ? { kind, value, label: `"${value}"` } : null;
  }
  return null;
}

// Split argv into the slug, the sources by kind, the limits and the phase switches.
export function parseArgs(argv) {
  const positional = [];
  const sources = { hashtags: [], users: [], keywords: [] };
  const out = {
    sourceLimit: DEFAULT_SOURCE_LIMIT,
    commentLimit: DEFAULT_COMMENT_LIMIT,
    sessions: null, // every ISP slot
    rate: DEFAULT_RATE,
    concurrency: DEFAULT_CONCURRENCY,
    minPlays: { ...DEFAULT_MIN_PLAYS },
    comments: true,
    download: true,
  };
  const value = (a, it) => (a.includes("=") ? a.slice(a.indexOf("=") + 1) : argv[++it.v]);
  const posInt = (raw, opt) => {
    const n = Number(raw);
    if (!Number.isInteger(n) || n < 1) throw new Error(`${opt} needs a positive integer`);
    return n;
  };
  const count = (raw, opt) => {
    const n = Number(raw);
    if (!Number.isInteger(n) || n < 0) throw new Error(`${opt} needs a whole number`);
    return n;
  };
  const add = (kind, key, raw) => {
    const source = sourceOf(kind, raw);
    if (!source) throw new Error(`--${kind} cannot use "${raw}"`);
    if (!sources[key].includes(source.value)) sources[key].push(source.value);
  };
  const is = (a, opt) => a === opt || a.startsWith(`${opt}=`);
  for (const it = { v: 0 }; it.v < argv.length; it.v++) {
    const a = argv[it.v];
    if (is(a, "--hashtag")) add("hashtag", "hashtags", value(a, it));
    else if (is(a, "--user")) add("user", "users", value(a, it));
    else if (is(a, "--keyword")) add("keyword", "keywords", value(a, it));
    else if (is(a, "--source-limit")) out.sourceLimit = posInt(value(a, it), "--source-limit");
    else if (is(a, "--comment-limit")) out.commentLimit = posInt(value(a, it), "--comment-limit");
    else if (is(a, "--sessions")) out.sessions = posInt(value(a, it), "--sessions");
    else if (is(a, "--rate")) out.rate = posInt(value(a, it), "--rate");
    else if (is(a, "--concurrency")) out.concurrency = posInt(value(a, it), "--concurrency");
    else if (is(a, "--hashtag-min-plays")) out.minPlays.hashtag = count(value(a, it), "--hashtag-min-plays");
    else if (is(a, "--out")) out.dir = value(a, it);
    else if (a === "--no-comments") out.comments = false;
    else if (a === "--no-download") out.download = false;
    else if (a.startsWith("--")) throw new Error(`Unknown option ${a}`);
    else positional.push(a);
  }
  return { slug: positional[0], sources, dir: join("docs", "intel", "tiktok", positional[0] ?? ""), ...out };
}

// The query params of a request the page itself sent, without its signatures, or null when the
// url is not an API call carrying the page's device id.
export function templateFrom(requestUrl) {
  const url = new URL(requestUrl);
  if (!url.pathname.startsWith("/api/") || !url.searchParams.has("device_id")) return null;
  return Object.fromEntries([...url.searchParams].filter(([k]) => !SIGN_KEYS.includes(k)));
}

export function apiUrl(base, path, params) {
  return `${API}${path}?${new URLSearchParams({ ...base, ...params })}`;
}

// The parsed answer of an API call. An empty body is a refusal that follows the IP (seen only on
// the pool's former German and French IPs, cause not established); a non-JSON
// body ("Access Denied") how its edge answers a session that went too fast.
export function parseBody(text) {
  if (!text) throw new Blocked("empty response", "ip");
  try {
    return JSON.parse(text);
  } catch {
    throw new Blocked("non-JSON response", "session");
  }
}

// The saved progress with any newly given sources added, or a fresh one. A saved file for another
// comment limit is refused rather than mixed.
export function openProgress(saved, sources, commentLimit) {
  if (saved == null) return { sources, commentLimit, runs: [], videos: {} };
  if (saved.commentLimit !== commentLimit) {
    throw new Error(`videos.out.json is for commentLimit ${saved.commentLimit}, not ${commentLimit}`);
  }
  const merged = {};
  for (const key of ["hashtags", "users", "keywords"]) {
    merged[key] = [...new Set([...(saved.sources?.[key] ?? []), ...sources[key]])];
  }
  return { ...saved, sources: merged };
}

// The held videos plus a fetched batch from one source: deduplicated by id (the fetched copy wins,
// its stats are newer), newest first, each recording every source that surfaced it. `fresh` is how
// many of the batch were not held before.
export function mergeVideos(held, got, label) {
  const byId = new Map(held.map((v) => [v.id, v]));
  let fresh = 0;
  for (const v of got) {
    const before = byId.get(v.id);
    if (!before) fresh++;
    byId.set(v.id, { ...v, sources: [...new Set([...(before?.sources ?? []), label])] });
  }
  const videos = [...byId.values()].sort((a, b) => Number(b.createTime) - Number(a.createTime));
  return { videos, fresh };
}

// The videos of a source's batch worth keeping: those with at least the source kind's play floor
// (a hashtag page mixes popular videos with fresh ones nobody watched) whose caption is English.
// TikTok labels a caption's language in `textLanguage`; "un" is a caption it could not tell, in
// practice hashtags alone, and stays.
export function keepVideos(source, items, minPlays) {
  return items.filter(
    (v) => (v.stats?.playCount ?? 0) >= minPlays[source.kind] && KEPT_LANGUAGES.includes(v.textLanguage ?? "un"),
  );
}

// The pulls a run makes: a hashtag page answers each request for it with a different sample of
// its videos, so it is pulled `hashtagPulls` times, by as many sessions at once; a user's timeline
// is the same every time, one pull, and a keyword search is pulled once. Each pull carries its own paging
// `state` and its `result`.
export function sourcePulls(sources, hashtagPulls) {
  return sources.flatMap((source) =>
    Array.from({ length: source.kind === "hashtag" ? hashtagPulls : 1 }, () => ({ source, state: {}, result: null })),
  );
}

// The pulls merged per source, in source order: the videos any pull yielded (deduplicated by id),
// how many pulls there were, whether every pull ended complete, whether TikTok knows the source.
export function mergePulls(pulls) {
  const bySource = new Map();
  for (const { source, state, result } of pulls) {
    const merged = bySource.get(source) ?? { source, items: new Map(), pulls: 0, complete: true, missing: false };
    for (const v of state.items ?? []) if (!merged.items.has(v.id)) merged.items.set(v.id, v);
    merged.pulls++;
    merged.complete &&= !!result?.complete;
    merged.missing ||= !!result?.missing;
    bySource.set(source, merged);
  }
  return [...bySource.values()].map((m) => ({ ...m, items: [...m.items.values()] }));
}

// The held comments plus a fetched batch, deduplicated by cid (the fetched copy wins), in thread order.
export function mergeComments(held, got) {
  const byId = new Map(held.map((c) => [c.cid, c]));
  for (const c of got) byId.set(c.cid, c);
  return [...byId.values()];
}

// The id a source's list endpoint pages by: a hashtag's challenge id, a user's secUid, or the
// keyword itself. Undefined when TikTok does not know the hashtag or user.
async function resolveSource(call, source) {
  if (source.kind === "keyword") return source.value;
  if (source.kind === "hashtag") {
    return (await call("challenge/detail/", { challengeName: source.value })).challengeInfo?.challenge?.id;
  }
  return (await call("user/detail/", { uniqueId: source.value, secUid: "" })).userInfo?.user?.secUid;
}

// The list request of each source kind, as the page itself sends it. A search pages by offset and
// carries the id of its first page (`state.searchId`) on the later ones.
const LISTS = {
  hashtag: (id, cursor) => ["challenge/item_list/", { challengeID: id, count: 30, cursor }],
  user: (id, cursor) => ["post/item_list/", { secUid: id, count: 35, cursor }],
  keyword: (id, cursor, state) => ["search/item/full/", { keyword: id, offset: cursor, ...(state.searchId && { search_id: state.searchId }) }],
};

// One page of a source's list: its videos and whether there are more. The search endpoint names
// its fields differently from the list endpoints and reports a refusal (a logged-out viewer) as a
// status code with no videos.
function listPage(source, body) {
  if (source.kind !== "keyword") return { list: body.itemList ?? [], more: !!body.hasMore };
  if (body.status_code) throw new AccountRefused(`search refused: ${body.status_code} ${body.status_msg ?? ""}`.trim());
  return { list: body.item_list ?? [], more: !!body.has_more };
}

// Page one source's videos into `state.items`. `state` carries the resolved id, the cursor and the
// items across calls, so when a call throws (a blocked session) another session continues from the
// same page. Ends complete at the last page, or when a hashtag page or a search only repeats
// videos already seen (a hashtag page does after a few hundred); ends incomplete at `limit`, and
// when a user's timeline repeats itself: TikTok gives an anonymous viewer one page of a profile
// (about 35 videos) and then answers every cursor with that page again. A source TikTok does not
// know is reported missing.
export async function collectSource(call, source, limit, state) {
  state.items ??= [];
  state.cursor ??= 0;
  state.stale ??= 0;
  state.id ??= await resolveSource(call, source);
  if (!state.id) return { complete: true, missing: true };
  const seen = new Set(state.items.map((v) => v.id));
  while (state.items.length < limit) {
    const body = await call(...LISTS[source.kind](state.id, state.cursor, state));
    const { list, more } = listPage(source, body);
    state.searchId ??= body.extra?.logid;
    const unseen = list.filter((v) => !seen.has(v.id));
    for (const v of unseen) {
      seen.add(v.id);
      state.items.push(v);
    }
    if (!more || !list.length) return { complete: true, missing: false };
    state.stale = unseen.length ? 0 : state.stale + 1;
    if (state.stale >= STALE_PAGES) return { complete: source.kind !== "user", missing: false };
    state.cursor = body.cursor;
  }
  return { complete: false, missing: false };
}

// How much a top-level comment was engaged with: its likes plus its replies.
const popularity = (c) => (c.digg_count ?? 0) + (c.reply_comment_total ?? 0);

// Collect one video's comments into `state.comments`, at most `limit` (top-level and replies
// together). First every top-level comment is paged (TikTok cannot sort them; its own order is
// only loosely by likes). Then the room left under the limit goes to replies, the most popular
// threads first, one page per thread in turn, so no single thread uses it up. The result holds
// each top-level comment, most popular first, followed by its replies. The top-level endpoint
// carries no replies; a comment's replies are paged separately, addressed by the video id.
// `state` carries the top-level comments, the cursors and the reply threads across calls, so when
// a call throws another session continues without refetching anything.
export async function collectComments(call, videoId, limit, state) {
  state.top ??= [];
  state.cursor ??= 0;
  state.threads ??= new Map(); // cid -> { replies, cursor, done }
  const ranked = () => [...state.top].sort((a, b) => popularity(b) - popularity(a));
  const held = () => state.top.length + [...state.threads.values()].reduce((n, t) => n + t.replies.length, 0);
  try {
    const seen = new Set(state.top.map((c) => c.cid));
    while (!state.topDone && state.top.length < limit) {
      const body = await call("comment/list/", { aweme_id: videoId, count: COMMENT_PAGE, cursor: state.cursor });
      const list = body.comments ?? [];
      for (const c of list) {
        if (seen.has(c.cid) || state.top.length >= limit) continue;
        seen.add(c.cid);
        state.top.push(c);
      }
      if (!body.has_more || !list.length) state.topDone = true;
      state.cursor = body.cursor;
    }
    const open = () =>
      ranked().filter((c) => c.reply_comment_total > 0 && !state.threads.get(c.cid)?.done);
    for (let round = open(); round.length && held() < limit; round = open()) {
      for (const c of round) {
        if (held() >= limit) break;
        if (!state.threads.has(c.cid)) state.threads.set(c.cid, { replies: [], cursor: 0, done: false });
        const thread = state.threads.get(c.cid);
        const body = await call("comment/list/reply/", {
          item_id: videoId,
          comment_id: c.cid,
          count: COMMENT_PAGE,
          cursor: thread.cursor,
        });
        const replies = body.comments ?? [];
        thread.replies.push(...replies.slice(0, limit - held()));
        thread.done = !body.has_more || !replies.length;
        thread.cursor = body.cursor;
      }
    }
    return { complete: true };
  } finally {
    state.comments = ranked().flatMap((c) => [c, ...(state.threads.get(c.cid)?.replies ?? [])]);
  }
}

// The videos whose comments this run fetches: those not yet recorded complete.
export function planComments(videos, progress) {
  return videos
    .filter((v) => !progress.videos[v.id]?.comments?.complete)
    .map((v) => ({ id: v.id, commentCount: v.stats?.commentCount ?? null, state: {} }));
}

// The videos this run downloads: those without a file on disk (`hasFile(id)`; the files live
// outside the repo, so the progress file alone cannot tell). A photo post has no video file.
export function planDownloads(videos, progress, hasFile) {
  return videos.filter((v) => !v.imagePost && !hasFile(v.id)).map((v) => ({ id: v.id }));
}

// Download one video through a session: ask TikTok for the video again (`call`; the play address
// in the held metadata is signed for the session that fetched it and has expired), fetch the bytes
// in the same session (`fetchBytes`) and `write` the file. Returns "downloaded", "gone" when
// TikTok no longer has the video, else "failed <status>" with the play address's answer.
export async function downloadVideo(unit, dir, { call, fetchBytes, write }) {
  const body = await call("item/detail/", { itemId: unit.id });
  const playAddr = body.itemInfo?.itemStruct?.video?.playAddr;
  if (!playAddr) return "gone";
  const { status, bytes } = await fetchBytes(playAddr);
  if (status !== 200) return `failed ${status}`;
  await write(join(dir, `${unit.id}.mp4`), bytes);
  return "downloaded";
}

// Where video files are kept: outside the repo, so they outlive a checkout or worktree.
export function videosDir(env = process.env) {
  const dir = env.TIKTOK_VIDEOS_DIR || "~/.local/share/tiktok";
  return dir.startsWith("~") ? join(homedir(), dir.slice(1)) : resolve(dir);
}

// A proxy url as the browser's { server, username, password }.
export function proxyDict(proxyUrl) {
  const u = new URL(proxyUrl);
  const proxy = { server: `${u.protocol}//${u.host}` };
  if (u.username) proxy.username = decodeURIComponent(u.username);
  if (u.password) proxy.password = decodeURIComponent(u.password);
  return proxy;
}

// Slot `slot` (1-based) of the ISP pool: the same endpoint on the base port + slot, one fixed IP each.
export function ispProxyAt(baseUrl, slot) {
  const u = new URL(baseUrl);
  u.port = String(Number(u.port) + slot);
  return u.toString().replace(/\/$/, "");
}

// `lanes` workers pull units off one queue, each through the session `open(lane)` gives it. When a
// unit's work throws (a blocked session, a dead page) the lane hands the session back with the
// error (`close(session, error)`), the unit goes back on the queue and the lane opens a session again;
// MAX_LANE_FAILURES failures in a row stop the lane. A throw on a session another lane already
// handed back (`close` answers false) is that lane's failure, not this one's. Every session opened is handed back. Returns
// how many units were left undone.
export async function drain({ lanes, units, open, close, work, log }) {
  const queue = [...units];
  await Promise.all(
    Array.from({ length: lanes }, async (_, lane) => {
      let session = null;
      let failures = 0;
      const failed = (what, e) => {
        failures++;
        log(`  [${lane}] ${what}: ${e.message.split("\n")[0]}`);
      };
      while (queue.length && failures < MAX_LANE_FAILURES) {
        if (!session) {
          try {
            session = await open(lane);
          } catch (e) {
            failed("session did not open", e);
            continue;
          }
        }
        const unit = queue.shift();
        if (unit === undefined) break;
        try {
          await work(session, unit);
          failures = 0;
        } catch (e) {
          queue.push(unit);
          const mine = (await close(session, e)) !== false;
          if (mine) failed(e instanceof Blocked ? "blocked, replacing the session" : "failed, replacing the session", e);
          session = null;
        }
      }
      if (session) await close(session, null);
      if (failures >= MAX_LANE_FAILURES) log(`  [${lane}] stopped after ${failures} failures in a row`);
    }),
  );
  return { left: queue.length };
}

// A gate that lets `rate` request starts through per second, evenly spaced: each start is at
// least 1000/rate ms after the previous one, and a pause earns no credit. Callers await it right
// before each request.
export function pacer(rate, now = Date.now, wait = sleep) {
  const gap = 1000 / rate;
  let next = 0;
  return async () => {
    const at = Math.max(now(), next);
    next = at + gap;
    await wait(at - now());
  };
}

// `count` sessions shared by any number of lanes: lane n works through the session of slot
// (n % count) + 1, opened when first asked for. A session handed back with an error is closed once,
// however many lanes report it; when the error is held against the IP (a Blocked with scope "ip")
// the slot's next session opens only after `cooldownMs`, otherwise at once. A session handed back
// without an error closes when its last lane lets go. `close` answers whether this call was the
// one that closed the session.
export function sharedSessions({ count, openOne, closeOne, cooldownMs, wait }) {
  const slots = Array.from({ length: count }, () => ({ entry: null, cooling: false }));
  return {
    async open(lane) {
      const slot = slots[lane % count];
      if (!slot.entry) {
        const entry = { users: 0, session: null };
        entry.opening = (async () => {
          if (slot.cooling) await wait(cooldownMs);
          slot.cooling = false;
          entry.session = await openOne((lane % count) + 1);
          return entry.session;
        })();
        slot.entry = entry;
      }
      const entry = slot.entry;
      entry.users++;
      try {
        return await entry.opening;
      } catch (e) {
        if (slot.entry === entry) slot.entry = null;
        throw e;
      }
    },
    async close(session, failed) {
      const slot = slots.find((s) => s.entry?.session === session);
      if (!slot) return false; // another lane already handed this session back as failed
      if (!failed && --slot.entry.users > 0) return false;
      slot.entry = null;
      slot.cooling = failed?.scope === "ip";
      await closeOne(session);
      return true;
    },
  };
}

// Whether a source is collected through the logged-in account: a keyword search always (TikTok
// answers it only to a logged-in viewer), a user's timeline when there is an account (an anonymous
// viewer gets one page of it), a hashtag page never.
export function usesAccount(source, hasAccount) {
  return source.kind === "keyword" || (source.kind === "user" && hasAccount);
}

const storeDir = (env) => env.SECRETS_MANAGER_STATE_PATH || join(homedir(), ".config", "secrets-manager");

// The TikTok account the secrets-manager skill logged in: the store's first active login, with the
// browser profile it lives in and the ISP slot it logged in from. Null when the store has none.
export function loadAccount(env = process.env) {
  let row;
  try {
    const db = new DatabaseSync(join(storeDir(env), "secrets.sqlite"), { readOnly: true });
    row = db.prepare("SELECT username, isp_slot FROM tiktok WHERE status = 'active' AND isp_slot IS NOT NULL ORDER BY username").get();
    db.close();
  } catch {
    return null;
  }
  return row ? { username: row.username, slot: row.isp_slot, profile: join(storeDir(env), "profiles", row.username) } : null;
}

// Record in the store that an account's session is gone, which is what `login tiktok` picks up
// (it checks the profile against tiktok.com before signing in again).
export function markAccountExpired(username, env = process.env) {
  const db = new DatabaseSync(join(storeDir(env), "secrets.sqlite"));
  db.prepare("UPDATE tiktok SET status = 'expired', updated_at = ? WHERE username = ?").run(new Date().toISOString(), username);
  db.close();
}

// One anonymous session: a Camoufox browser on one ISP slot (a fixed IP).
const openSession = (slot, rate) => openSessionOn(slot, rate, {});

// The account's session: the browser profile it logged in with (the same device to TikTok), on its
// ISP slot. A profile that is no longer signed in marks the account expired.
async function openAccountSession(account) {
  const session = await openSessionOn(account.slot, ACCOUNT_RATE, {
    user_data_dir: account.profile,
    fingerprint: JSON.parse(readFileSync(join(account.profile, "fingerprint.json"), "utf8")),
    i_know_what_im_doing: true, // the fingerprint is the one the profile logged in with
  });
  const cookies = await session.page.context().cookies("https://www.tiktok.com");
  if (!cookies.some((c) => c.name === SESSION_COOKIE && c.value)) {
    await closeSession(session);
    markAccountExpired(account.username);
    throw new Error(`${account.username} is logged out; run the secrets-manager skill's \`login tiktok\``);
  }
  return session;
}

// A session: Camoufox on one ISP slot, on tiktok.com, with the page's request template captured.
// Media, images and fonts are not loaded and audio is muted. `launch` adds to the browser's options.
async function openSessionOn(slot, rate, launch) {
  if (!process.env.ISP_PROXY_URL) throw new Error("No ISP_PROXY_URL in ~/.config/intel/.env.");
  const proxyUrl = ispProxyAt(process.env.ISP_PROXY_URL, slot);
  const { Camoufox } = await import("camoufox-js");
  const browser = await Camoufox({
    headless: true,
    geoip: true, // timezone and locale follow the exit IP
    proxy: proxyDict(proxyUrl),
    main_world_eval: true, // API calls run in the page's own world, where TikTok's script signs fetch()
    firefox_user_prefs: { "media.volume_scale": "0.0" },
    ...launch,
  });
  try {
    const page = await browser.newPage();
    await page.route("**/*", (route, request) =>
      ["media", "image", "font"].includes(request.resourceType()) ? route.abort() : route.continue(),
    );
    let base = null;
    page.on("request", (request) => {
      base ??= templateFrom(request.url());
    });
    await page.goto(START_URL, { waitUntil: "domcontentloaded", timeout: 60000 });
    for (let waited = 0; !base && waited < TEMPLATE_WAIT_MS; waited += 500) await sleep(500);
    if (!base) throw new Blocked("the page sent no API request");
    return { browser, page, base, turn: pacer(rate) };
  } catch (e) {
    await browser.close().catch(() => {});
    throw e;
  }
}

const closeSession = (session) => session.browser.close().catch(() => {});

// One API call through a session, in its turn at the session's rate. A request that hangs on a dead exit is aborted, which throws
// like any other failed call. A session's first call sometimes comes back empty before TikTok's
// script is ready and the same session then works, so an empty answer is asked once more, a
// moment later, before the session is taken as flagged.
async function callApi(session, path, params) {
  const url = apiUrl(session.base, path, params);
  const ask = async () => {
    await session.turn();
    return session.page.evaluate(
      `mw:fetch(${JSON.stringify(url)}, { credentials: "include", signal: AbortSignal.timeout(${REQUEST_TIMEOUT_MS}) }).then((r) => r.text())`,
    );
  };
  return parseBody((await ask()) || (await sleep(EMPTY_RETRY_MS), await ask()));
}

// One video file through a session, in its turn like an API call: fetched inside the page (the
// play address is signed for this session's IP) and handed out as bytes with the status. A fetch
// that fails or times out is status 0 (this video failed; the session is fine); a file over the
// size cap is status 413.
async function fetchBytesIn(session, url) {
  await session.turn();
  const { status, b64 } = await session.page.evaluate(
    `mw:fetch(${JSON.stringify(url)}, { credentials: "include", signal: AbortSignal.timeout(${DOWNLOAD_TIMEOUT_MS}) }).then(async (r) => {
      if (r.status !== 200) return { status: r.status };
      const u = new Uint8Array(await r.arrayBuffer());
      if (u.length > ${MAX_DOWNLOAD_BYTES}) return { status: 413 };
      let s = "";
      for (let i = 0; i < u.length; i += 0x8000) s += String.fromCharCode.apply(null, u.subarray(i, i + 0x8000));
      return { status: r.status, b64: btoa(s) };
    }).catch(() => ({ status: 0 }))`,
  );
  return { status, bytes: b64 ? Buffer.from(b64, "base64") : null };
}

async function loadJson(path, fallback) {
  try {
    return JSON.parse(await readFile(path, "utf8"));
  } catch {
    return fallback;
  }
}

async function loadJsonl(path) {
  try {
    return (await readFile(path, "utf8")).split("\n").filter(Boolean).map((l) => JSON.parse(l));
  } catch {
    return [];
  }
}

const jsonl = (rows) => (rows.length ? rows.map((r) => JSON.stringify(r)).join("\n") + "\n" : "");

const ispCount = () => Math.max(1, Number(process.env.ISP_PROXY_COUNT) || 1);

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const dir = args.dir;
  const progressPath = join(dir, "videos.out.json");
  const videosPath = join(dir, "videos.jsonl");
  const progress = args.slug ? openProgress(await loadJson(progressPath, null), args.sources, args.commentLimit) : null;
  const sources = progress && [
    ...progress.sources.hashtags.map((v) => sourceOf("hashtag", v)),
    ...progress.sources.users.map((v) => sourceOf("user", v)),
    ...progress.sources.keywords.map((v) => sourceOf("keyword", v)),
  ];
  if (!sources?.length) {
    console.error(
      "Usage: node " +
        "${CLAUDE_PLUGIN_ROOT}/skills/fetch-tiktok-mentions/scripts/fetch-tiktok-mentions.mjs <slug> " +
        "[--hashtag <name>]... [--user <handle>]... [--keyword <words>]... " +
        "[--hashtag-min-plays <n>] " +
        "[--source-limit <n>] [--comment-limit <n>] [--sessions <n>] [--concurrency <n>] [--out <dir>] [--no-comments] [--no-download]",
    );
    process.exit(1);
  }
  requireEnv("ISP_PROXY_URL");
  const log = (line) => process.stderr.write(line + "\n");
  await mkdir(join(dir, "comments"), { recursive: true });
  await mkdir(videosDir(), { recursive: true });

  const thisRun = { at: new Date().toISOString(), fetched: 0, kept: 0, new: 0, sources: {} };
  progress.runs.push(thisRun);
  let videos = await loadJsonl(videosPath);

  // Lanes finish units concurrently; every file write goes through one chain so the progress file
  // is never torn.
  let checkpoint = Promise.resolve();
  const commit = (fn) => (checkpoint = checkpoint.then(fn));
  const saveProgress = () => writeFile(progressPath, JSON.stringify(progress, null, 1));
  const entry = (id) => (progress.videos[id] ??= { comments: { count: 0, complete: false }, downloaded: false });

  // Each phase works through one session per ISP slot, several lanes on each.
  const sessions = Math.min(args.sessions ?? ispCount(), ispCount());
  const pool = async (units, work, perSession = args.concurrency) => {
    const shared = sharedSessions({
      count: sessions,
      openOne: (slot) => openSession(slot, args.rate),
      closeOne: closeSession,
      cooldownMs: SESSION_COOLDOWN_MS,
      wait: sleep,
    });
    const lanes = Math.min(sessions * perSession, units.length);
    return drain({ lanes, units, open: shared.open, close: shared.close, work, log });
  };
  let left = 0;

  // 1. Collect every source's videos: a hashtag through several sessions at once, a user and a
  //    keyword once. The pulls that need the account go through its one session, one after another.
  const account = loadAccount();
  log(
    `${args.slug}: collecting ${sources.length} source(s) through ${sessions} session(s)` +
      (account ? ` and the account ${account.username}` : ""),
  );
  const pulls = sourcePulls(sources, Math.min(sessions, HASHTAG_PULLS));
  const collect = async (session, pull) => {
    const call = (path, params) => callApi(session, path, params);
    pull.result = await collectSource(call, pull.source, args.sourceLimit, pull.state);
  };
  const viaAccount = pulls.filter((pull) => usesAccount(pull.source, !!account));
  const collected = await Promise.all([
    pool(pulls.filter((pull) => !viaAccount.includes(pull)), collect),
    account
      ? drain({
          lanes: 1,
          units: viaAccount,
          open: () => openAccountSession(account),
          close: (session, error) => {
            if (error instanceof AccountRefused) markAccountExpired(account.username);
            return closeSession(session);
          },
          work: collect,
          log,
        })
      : { left: viaAccount.length },
  ]);
  if (!account && viaAccount.length) {
    log("  no logged-in TikTok account: keywords are not collected; run the secrets-manager skill's `login tiktok`");
  }
  left += collected[0].left + collected[1].left;
  for (const { source, items, pulls: count, complete, missing } of mergePulls(pulls)) {
    const kept = keepVideos(source, items, args.minPlays);
    const merged = mergeVideos(videos, kept, source.label);
    videos = merged.videos;
    const stats = { fetched: items.length, kept: kept.length, new: merged.fresh };
    thisRun.sources[source.label] = { ...stats, complete, pulls: count };
    for (const key of ["fetched", "kept", "new"]) thisRun[key] += stats[key];
    log(
      `  ${source.label}: ${items.length} videos in ${count} pull(s), ${kept.length} kept, ${merged.fresh} new` +
        (missing ? " (TikTok does not know it)" : complete ? "" : " (incomplete)"),
    );
  }
  for (const v of videos) entry(v.id);
  await writeFile(videosPath, jsonl(videos));
  await saveProgress();

  // 2. Comments, for the videos whose comments are not complete.
  if (args.comments) {
    const units = planComments(videos, progress);
    log(`${args.slug}: comments for ${units.length} video(s), up to ${args.commentLimit} each`);
    const save = (unit, complete) =>
      commit(async () => {
        const path = join(dir, "comments", `${unit.id}.jsonl`);
        const merged = mergeComments(await loadJsonl(path), unit.state.comments ?? []);
        await writeFile(path, jsonl(merged));
        entry(unit.id).comments = { count: merged.length, complete };
        await saveProgress();
      });
    const { left: undone } = await pool(units, async (session, unit) => {
      // A video TikTok reports no comments on is settled without a request.
      if (unit.commentCount !== 0) {
        const call = (path, params) => callApi(session, path, params);
        await collectComments(call, unit.id, args.commentLimit, unit.state);
      }
      unit.saved = true;
      await save(unit, true);
      log(`  ${unit.id}: ${unit.state.comments?.length ?? 0} comments`);
    });
    // What an unfinished video already yielded is kept; the next run fetches it again and merges.
    for (const unit of units) if (!unit.saved && unit.state.comments?.length) await save(unit, false);
    left += undone;
  }

  // 3. Video files, inside the sessions.
  if (args.download) {
    const hasFile = (id) => existsSync(join(videosDir(), `${id}.mp4`));
    // A file fetched earlier for another slug counts as this slug's download too.
    for (const v of videos) if (hasFile(v.id)) entry(v.id).downloaded = true;
    const units = planDownloads(videos, progress, hasFile);
    log(`${args.slug}: downloading ${units.length} video(s)`);
    const { left: undone } = await pool(
      units,
      async (session, unit) => {
        const outcome = await downloadVideo(unit, videosDir(), {
          call: (path, params) => callApi(session, path, params),
          fetchBytes: (url) => fetchBytesIn(session, url),
          write: writeFile,
        });
        if (outcome.startsWith("failed")) left++;
        await commit(async () => {
          entry(unit.id).downloaded = outcome === "downloaded";
          await saveProgress();
        });
        log(`  ${unit.id}: ${outcome.startsWith("failed") ? `download ${outcome}` : outcome}`);
      },
      DOWNLOAD_LANES,
    );
    left += undone;
  }

  await checkpoint;
  if (left) process.exitCode = 1; // units still undone; rerun to continue
  const all = Object.values(progress.videos);
  console.log(
    `${args.slug}: ${videos.length} videos (${thisRun.new} new this run), ` +
      `${all.filter((v) => v.comments.complete).length} with comments complete, ` +
      `${all.filter((v) => v.downloaded).length} downloaded, ${left} unit(s) left -> ${dir}, video files -> ${videosDir()}`,
  );
}

if (isMain) await main();
