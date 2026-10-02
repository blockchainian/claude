#!/usr/bin/env node
// ABOUTME: Profiles one X account from its own timeline (tweets.jsonl + replies.jsonl): volume, reply
// ABOUTME: ratio, cadence, posting hours, who it talks to, the assets and links it pushes, engagement, labels.
//
// Usage: profile-user.mjs <user-dir> [--labels <labels.jsonl>] [--out <dir>]
//        <user-dir> is docs/intel/x/kols/<user>/ holding tweets.jsonl and replies.jsonl.
//        Writes <out>/profile.json (default the user dir) and prints the summary a profile.md is
//        written from. --labels folds in the kol-spec labels: kind, topic, asset with stance, interest.
//
// Deterministic only: every number here comes from the archive. The qualitative read (who this
// account is, what moves them) is the model's job, from this JSON plus the representative posts.

import { readFileSync, writeFileSync } from "node:fs";
import { basename, join } from "node:path";

const engOf = (t) => ["likes", "retweets", "replies", "quotes"].reduce((s, k) => s + (Number(t[k]) || 0), 0);
const dayOf = (t) => new Date(t.created_at).toISOString().slice(0, 10);
const hourOf = (t) => new Date(t.created_at).getUTCHours();

// $CASHTAGS a post pushes (upper-cased, 2–10 chars), the clearest signal of what an account shills.
export function cashtags(text) {
  return [...(text || "").matchAll(/\$([A-Za-z][A-Za-z0-9]{1,9})\b/g)].map((m) => m[1].toUpperCase());
}
// @handles a post talks to or at, minus the author itself.
export function mentions(text, self) {
  const s = (self || "").toLowerCase();
  return [...(text || "").matchAll(/@(\w{1,15})/g)].map((m) => m[1].toLowerCase()).filter((h) => h !== s);
}
export function hashtags(text) {
  return [...(text || "").matchAll(/#(\w{1,30})/g)].map((m) => m[1].toLowerCase());
}
// The registrable host of each link a post carries (t.co is X's wrapper, dropped as noise).
export function domains(t) {
  return (t.urls || (t.text || "").match(/https?:\/\/\S+/g) || [])
    .map((u) => { try { return new URL(u.expanded_url ?? u.url ?? u).host.replace(/^www\./, ""); } catch { return null; } })
    .filter((h) => h && h !== "t.co");
}

const tally = (items) => { const m = new Map(); for (const x of items) m.set(x, (m.get(x) || 0) + 1); return m; };
const topN = (map, n) => [...map.entries()].sort((a, b) => b[1] - a[1]).slice(0, n).map(([k, v]) => ({ value: k, count: v }));
const median = (xs) => { if (!xs.length) return 0; const s = xs.slice().sort((a, b) => a - b); const m = s.length >> 1; return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2; };

// The whole profile of one account, computed from its own posts. `tweets` are its own posts,
// `replies` its replies to others; both carry the same fields as the archive lines.
export function profileStats(user, tweets, replies, labels = new Map()) {
  const all = [...tweets, ...replies];
  const days = new Set(all.map(dayOf));
  const dates = all.map((t) => t.created_at).sort((a, b) => Date.parse(a) - Date.parse(b));
  const hours = new Array(24).fill(0);
  for (const t of all) hours[hourOf(t)]++;
  const engs = all.map(engOf);
  const langs = tally(all.map((t) => t.lang || "und"));
  const topPosts = all.slice().sort((a, b) => engOf(b) - engOf(a)).slice(0, 15).map((t) => ({
    date: dayOf(t), eng: engOf(t), likes: t.likes ?? 0, lang: t.lang, text: t.text,
    url: t.url || `https://x.com/${t.author}/status/${t.id}`,
    label: labels.get(t.id) ? { kind: labels.get(t.id).kind, topic: labels.get(t.id).topic, asset: labels.get(t.id).asset, stance: labels.get(t.id).stance, interest: labels.get(t.id).interest } : null,
  }));
  // Label tallies (kol-spec fields): kind, topic, stance and interest as counts; asset with its stance split.
  const labelTally = { kind: new Map(), topic: new Map(), stance: new Map(), interest: new Map() };
  const assetTally = new Map();
  let labeled = 0, about = 0;
  for (const t of all) {
    const l = labels.get(t.id);
    if (!l) continue;
    labeled++;
    if (l.about) about++;
    for (const f of Object.keys(labelTally)) if (l[f] && l[f] !== "none") labelTally[f].set(l[f], (labelTally[f].get(l[f]) || 0) + 1);
    if (l.stance === "none") labelTally.stance.set("none", (labelTally.stance.get("none") || 0) + 1);
    if (l.asset && l.asset !== "none") {
      const a = assetTally.get(l.asset) || { count: 0, bullish: 0, bearish: 0, neutral: 0 };
      a.count++;
      if (l.stance in a) a[l.stance]++;
      assetTally.set(l.asset, a);
    }
  }
  const topAssets = [...assetTally.entries()].sort((a, b) => b[1].count - a[1].count).slice(0, 20).map(([value, a]) => ({ value, ...a }));
  return {
    user,
    posts: all.length,
    tweets: tweets.length,
    replies: replies.length,
    reply_ratio: all.length ? Math.round((replies.length / all.length) * 1000) / 1000 : 0,
    active_days: days.size,
    span_days: dates.length ? Math.round((Date.parse(dates.at(-1)) - Date.parse(dates[0])) / 86400000) + 1 : 0,
    posts_per_active_day: days.size ? Math.round((all.length / days.size) * 10) / 10 : 0,
    first_post: dates[0] ?? null,
    last_post: dates.at(-1) ?? null,
    hours_utc: hours,
    engagement: { total: engs.reduce((a, b) => a + b, 0), avg: all.length ? Math.round((engs.reduce((a, b) => a + b, 0) / all.length) * 10) / 10 : 0, median: median(engs), max: Math.max(0, ...engs) },
    top_cashtags: topN(tally(all.flatMap((t) => cashtags(t.text))), 20),
    top_mentions: topN(tally(all.flatMap((t) => mentions(t.text, t.author))), 20),
    top_hashtags: topN(tally(all.flatMap((t) => hashtags(t.text))), 15),
    top_domains: topN(tally(all.flatMap(domains)), 15),
    languages: topN(langs, 8),
    labels: { labeled, about, kind: topN(labelTally.kind, 8), topic: topN(labelTally.topic, 15), asset: topAssets, stance: topN(labelTally.stance, 4), interest: topN(labelTally.interest, 10) },
    top_posts: topPosts,
  };
}

function readJsonl(path) {
  try { return readFileSync(path, "utf8").split("\n").filter((l) => l.trim()).map((l) => JSON.parse(l)); }
  catch { return []; }
}

function main() {
  const args = process.argv.slice(2);
  const opt = (k) => { const i = args.indexOf(k); return i >= 0 ? args[i + 1] : null; };
  const dir = args.find((a, i) => !a.startsWith("--") && !args[i - 1]?.startsWith("--"));
  if (!dir) { console.error("usage: profile-user.mjs <user-dir> [--labels <labels.jsonl>] [--out <dir>]"); process.exit(1); }
  const user = basename(dir.replace(/\/$/, ""));
  const tweets = readJsonl(join(dir, "tweets.jsonl"));
  const replies = readJsonl(join(dir, "replies.jsonl"));
  if (!tweets.length && !replies.length) { console.error(`no posts under ${dir}`); process.exit(1); }
  const labelsPath = opt("--labels");
  const labels = labelsPath
    ? new Map(readJsonl(labelsPath).map((x) => [x.id, x]))
    : new Map();
  const stats = profileStats(user, tweets, replies, labels);
  const out = opt("--out") || dir;
  writeFileSync(join(out, "profile.json"), JSON.stringify(stats, null, 1));

  const busiest = stats.hours_utc.map((c, h) => [h, c]).sort((a, b) => b[1] - a[1]).slice(0, 4).map(([h]) => `${h}:00`);
  console.log(`@${user}: ${stats.posts} posts (${stats.tweets} tweets, ${stats.replies} replies, reply ratio ${stats.reply_ratio})`);
  console.log(`  ${stats.active_days} active days over ${stats.span_days}, ${stats.posts_per_active_day}/active day; busiest UTC hours ${busiest.join(", ")}`);
  console.log(`  engagement avg ${stats.engagement.avg}, median ${stats.engagement.median}, max ${stats.engagement.max}`);
  console.log(`  cashtags: ${stats.top_cashtags.slice(0, 8).map((c) => `$${c.value}(${c.count})`).join(" ") || "none"}`);
  console.log(`  talks to: ${stats.top_mentions.slice(0, 8).map((m) => `@${m.value}(${m.count})`).join(" ") || "none"}`);
  console.log(`  languages: ${stats.languages.map((l) => `${l.value}(${l.count})`).join(" ")}`);
  if (stats.labels.labeled) {
    console.log(`  labeled ${stats.labels.labeled}/${stats.posts}, ${stats.labels.about} with content; kinds ${stats.labels.kind.map((k) => `${k.value}(${k.count})`).join(" ")}`);
    console.log(`  topics ${stats.labels.topic.slice(0, 8).map((t) => `${t.value}(${t.count})`).join(" ")}`);
    console.log(`  assets ${stats.labels.asset.slice(0, 8).map((a) => `${a.value}(${a.count}: +${a.bullish}/-${a.bearish})`).join(" ") || "none"}`);
    console.log(`  interests ${stats.labels.interest.map((i) => `${i.value}(${i.count})`).join(" ") || "none"}`);
  }
  console.log(`-> ${join(out, "profile.json")}`);
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split("/").pop())) main();
