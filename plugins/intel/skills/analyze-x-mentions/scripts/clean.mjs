#!/usr/bin/env node
// ABOUTME: Cleans a fetch-x-mentions tweets.jsonl: drops bot alert templates, mass-tag posts,
// ABOUTME: near-duplicates, stubs and automated-spam accounts, then prints the corpus facts the reception doc opens with.
//
// Usage: clean.mjs <tweets.jsonl> --out <clean.json> [--since YYYY-MM-DD] [--until YYYY-MM-DD] [--bot-pattern <regex>] [--keep-bots]
import { readFileSync, writeFileSync, existsSync } from "node:fs";

// Token-alert bot templates and DM-spam; extend per corpus with --bot-pattern.
const BOT_DEFAULT =
  "Route: |Venue: |Launch: |MIGRATION|Migration |CTO SIGNAL|CTO ALERT|WALLET FLOW CHECK|Quick Buy|Quick Swap|CHECK EVENTS|Volume Alert|dm us";
const MAX_TAGS = 6;
const MIN_LEN = 8;

// An automated-spam / fake ACCOUNT (not just a spam post): it posts at high volume yet gets no
// real distribution — its typical post earns almost no impressions (views), few likes, and
// hardly anyone ever @-mentions it back. Bought-follower and shadow-banned accounts look exactly
// like this: a big follower count but a median post seen by only a few hundred people (e.g.
// @qkl2058: ~88k followers, ~1.2k median views, ~4 likes). The archive has no follower count, so
// we use impressions directly. This catches referral/scanner/alert bots (SpinoPepe, YutiBot,
// qkl2058, MrbossOfficiial…) while sparing real accounts that get distributed or replied to
// (@betaai, @haze0x, @aixbt_agent, active KOLs and users).
const BOT_MIN_POSTS = 50; // only judge accounts with enough volume to be sure
const BOT_MAX_MED_VIEWS = 2500; // a real account's median post is seen by more than this...
const BOT_MAX_INBOUND_RATIO = 0.01; // ...unless real people @-mention it (distinct repliers > 1% of posts)
const BOT_INBOUND_FLOOR = 5; // always allow a few repliers, so small samples are safe
const BOT_MAX_AVG_LIKES = 8; // and its posts average fewer likes than this
const BOT_DEAD_VIEWS = 150; // a median post seen by almost nobody...
const BOT_DEAD_LIKES = 2; // ...with almost no likes is dead reach even if it has some inbound

// With a follower count (fetched separately; the archive has none), two sharper rules that read X's
// own promotion signal — a real account's posts reach a fraction of its followers:
//   Rule 1 (demoted / bought followers): many followers but a median post seen by almost none of them.
//   Rule 2 (automated): few followers yet a huge lifetime post count (posts far more than anyone follows).
const BOT_FAKE_MIN_FOLLOWERS = 5000; // Rule 1 only judges accounts big enough for the ratio to mean something
const BOT_FAKE_VIEWS_PER_FOLLOWER = 0.015; // median views under 1.5% of followers = demoted...
const BOT_FAKE_MAX_VIEWS = 2000; // ...and still a low absolute reach (spares big accounts pulling thousands of views)
const BOT_AUTO_MAX_FOLLOWERS = 3000; // Rule 2: "few followers"
const BOT_AUTO_TWEETS_PER_FOLLOWER = 8; // lifetime tweets ≥ 8× followers = automated posting
// A follower-based rule can misfire on a genuinely engaged-with account whose posts in THIS corpus
// are low-view replies. Real human traction — many distinct accounts @-mentioning it — vetoes
// the follower rules; the view-only base rules (which have their own inbound gate) still apply.
const BOT_ENGAGED_INBOUND = 30;
const BOT_ENGAGED_RATIO = 0.2;

const viewsOf = (t) => { const v = parseInt(t.views, 10); return Number.isFinite(v) ? v : 0; };
const median = (a) => { const s = [...a].sort((x, y) => x - y), m = s.length >> 1; return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2; };

// The set of automated-spam / fake account handles among these posts. `followers`, when given, maps a
// handle to { followers, tweets, status }; status 404 means X suspended the account (itself a fake tell).
export function botAuthors(posts, followers = null) {
  const likes = {}, views = {}, inbound = {};
  for (const t of posts) {
    const a = t.author;
    if (!a) continue;
    (likes[a] ||= []).push(t.likes || 0);
    (views[a] ||= []).push(viewsOf(t));
    for (const m of t.text.matchAll(/@(\w{1,15})/g)) {
      const h = m[1].toLowerCase();
      if (h !== a.toLowerCase()) (inbound[h] ||= new Set()).add(a);
    }
  }
  const bots = new Set();
  for (const a of Object.keys(likes)) {
    const n = likes[a].length;
    if (n < BOT_MIN_POSTS) continue;
    const medViews = median(views[a]);
    const avgLikes = likes[a].reduce((x, y) => x + y, 0) / n;
    const reach = inbound[a.toLowerCase()]?.size || 0;
    const lowReach = medViews < BOT_MAX_MED_VIEWS && reach <= Math.max(BOT_INBOUND_FLOOR, n * BOT_MAX_INBOUND_RATIO) && avgLikes < BOT_MAX_AVG_LIKES;
    const deadReach = medViews < BOT_DEAD_VIEWS && avgLikes < BOT_DEAD_LIKES;
    let fake = false, automated = false, suspended = false;
    const f = followers?.[a];
    const engaged = reach >= Math.max(BOT_ENGAGED_INBOUND, n * BOT_ENGAGED_RATIO);
    if (f) {
      suspended = f.status === 404;
      if (f.followers && !engaged) {
        fake = f.followers >= BOT_FAKE_MIN_FOLLOWERS && medViews < f.followers * BOT_FAKE_VIEWS_PER_FOLLOWER && medViews < BOT_FAKE_MAX_VIEWS;
        automated = f.followers < BOT_AUTO_MAX_FOLLOWERS && f.tweets && f.tweets / f.followers >= BOT_AUTO_TWEETS_PER_FOLLOWER;
      }
    }
    if (lowReach || deadReach || fake || automated || suspended) bots.add(a);
  }
  return bots;
}

// The tweets in a fetch-x-mentions log, one JSON object per line, deduplicated by id
// (a fill run may append a tweet already present; the last line wins).
export function readLog(path) {
  const byId = new Map();
  for (const line of readFileSync(path, "utf8").split("\n")) {
    if (!line.trim()) continue;
    const t = JSON.parse(line);
    byId.set(t.id, t);
  }
  return [...byId.values()];
}

export function dayOf(t) {
  return new Date(t.created_at).toISOString().slice(0, 10);
}

export function clean(tweets, botPattern = BOT_DEFAULT, since = null, until = null, dropBots = true, followers = null) {
  const botRe = new RegExp(botPattern, "i");
  const seen = new Set();
  const out = [];
  const dropped = { outOfRange: 0, bot: 0, massTag: 0, dupe: 0, short: 0, botAccount: 0 };
  for (const t of tweets) {
    const d = dayOf(t);
    if ((since && d < since) || (until && d >= until)) { dropped.outOfRange++; continue; }
    if (botRe.test(t.text)) { dropped.bot++; continue; }
    if ((t.text.match(/@\w+/g) || []).length >= MAX_TAGS) { dropped.massTag++; continue; }
    const key = t.text.replace(/@\w+/g, "").replace(/https?:\S+/g, "").replace(/\s+/g, " ").trim().toLowerCase();
    if (key.length < MIN_LEN) { dropped.short++; continue; }
    if (seen.has(key)) { dropped.dupe++; continue; }
    seen.add(key);
    out.push(t);
  }
  if (!dropBots) return { clean: out, dropped, botAccounts: 0 };
  // Second pass: drop every post from an automated-spam / fake account. Judge the account on its
  // whole in-window footprint (raw, before text-cleaning) — an engagement farm's "gm"/duplicate
  // filler is exactly what marks it, so dropping those first would hide it.
  const windowed = tweets.filter((t) => { const d = dayOf(t); return !((since && d < since) || (until && d >= until)); });
  const bots = botAuthors(windowed, followers);
  const kept = out.filter((t) => !bots.has(t.author));
  dropped.botAccount = out.length - kept.length;
  return { clean: kept, dropped, botAccounts: bots.size };
}

export function facts(raw, cleaned) {
  const days = [...new Set(cleaned.map(dayOf))].sort();
  const byDay = {};
  const byAuthor = {};
  for (const t of cleaned) {
    byDay[dayOf(t)] = (byDay[dayOf(t)] || 0) + 1;
    byAuthor[t.author] = (byAuthor[t.author] || 0) + 1;
  }
  return {
    raw: raw.length,
    clean: cleaned.length,
    rawAuthors: new Set(raw.map((t) => t.author)).size,
    from: days[0],
    to: days.at(-1),
    days: days.length,
    topAuthors: Object.entries(byAuthor).sort((a, b) => b[1] - a[1]).slice(0, 12),
    byDay,
  };
}

function main() {
  const args = process.argv.slice(2);
  const input = args.find((a) => !a.startsWith("--"));
  const out = args[args.indexOf("--out") + 1];
  const opt = (k) => (args.includes(k) ? args[args.indexOf(k) + 1] : null);
  const pat = opt("--bot-pattern") ?? BOT_DEFAULT;
  if (!input || !out) {
    console.error("Usage: clean.mjs <tweets.jsonl> --out <clean.json> [--since YYYY-MM-DD] [--until YYYY-MM-DD] [--bot-pattern <regex>] [--keep-bots]");
    process.exit(1);
  }
  const raw = readLog(input);
  const followersPath = opt("--followers");
  const followers = followersPath && existsSync(followersPath) ? JSON.parse(readFileSync(followersPath, "utf8")) : null;
  const { clean: cleaned, dropped, botAccounts } = clean(raw, pat, opt("--since"), opt("--until"), !args.includes("--keep-bots"), followers);
  writeFileSync(out, JSON.stringify(cleaned));
  console.log(JSON.stringify({ ...facts(raw, cleaned), dropped, botAccounts }, null, 1));
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split("/").pop())) main();
