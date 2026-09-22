#!/usr/bin/env node
// ABOUTME: Merges the per-post labels: sentiment over all posts and over the top-liked, like/dislike
// ABOUTME: counts per topic, interested-party share of praise, and id/quote checks for every summary.
//
// Usage: aggregate.mjs <clean.json> <dir-with-summary*.txt> --labels <labels.jsonl> [--since YYYY-MM-DD] [--until YYYY-MM-DD] [--top 300]
//        (without --labels it reads the labelsN.json files in <dir>; --timeline prints the top 3 posts of every day instead)
import { readFileSync, readdirSync, existsSync } from "node:fs";
import { join } from "node:path";
import { dayOf } from "./clean.mjs";

export function readLabels(path) {
  return readFileSync(path, "utf8").split("\n").filter((l) => l.trim()).map((l) => JSON.parse(l));
}

export function inWindow(label, byId, since = null, until = null) {
  const t = byId.get(label.id);
  if (!t) return false;
  const d = dayOf(t);
  return (!since || d >= since) && (!until || d < until);
}

export const norm = (s) =>
  s.replace(/\s+/g, " ").replace(/[’‘]/g, "'").replace(/[“”]/g, '"').replace(/&amp;/g, "&")
    .toLowerCase().replace(/[.!?]+$/, "").trim();

export function timeline(tweets, perDay = 3) {
  const byDay = {};
  for (const t of tweets) (byDay[dayOf(t)] = byDay[dayOf(t)] || []).push(t);
  return Object.keys(byDay).sort().map((d) => ({
    day: d, n: byDay[d].length, top: byDay[d].sort((a, b) => b.likes - a.likes).slice(0, perDay),
  }));
}

export function tally(rows) {
  const o = {};
  for (const r of rows) o[r.sentiment] = (o[r.sentiment] || 0) + 1;
  return o;
}

export function byTopic(rows, byId) {
  const n = {}, like = {}, dislike = {}, months = {};
  for (const r of rows) {
    const t = r.topic || "none";
    n[t] = (n[t] || 0) + 1;
    if (r.sentiment === "like") like[t] = (like[t] || 0) + 1;
    if (r.sentiment === "dislike") dislike[t] = (dislike[t] || 0) + 1;
    const p = byId.get(r.id);
    if (p) { const m = dayOf(p).slice(0, 7); (months[t] = months[t] || {})[m] = (months[t][m] || 0) + 1; }
  }
  return Object.entries(n).sort((a, b) => b[1] - a[1]).map(([topic, count]) => ({
    topic, count, like: like[topic] || 0, dislike: dislike[topic] || 0,
    peak: Object.entries(months[topic] || {}).sort((a, b) => b[1] - a[1])[0]?.[0] ?? null,
  }));
}

export function byFeature(rows, byId) {
  const n = {}, authors = {};
  for (const r of rows) {
    n[r.feature] = (n[r.feature] || 0) + 1;
    (authors[r.feature] = authors[r.feature] || new Set()).add(byId.get(r.id)?.author);
  }
  return Object.entries(n).sort((a, b) => b[1] - a[1]).map(([feature, count]) => ({ feature, count, authors: authors[feature].size }));
}

// The most repeated short texts (points or requests), case-folded, for ranking findings by count.
export function topTexts(rows, key, limit = 25) {
  const n = {};
  for (const r of rows) {
    const t = (r[key] || "").trim().toLowerCase();
    if (t) n[t] = (n[t] || 0) + 1;
  }
  return Object.entries(n).sort((a, b) => b[1] - a[1]).slice(0, limit).map(([text, count]) => ({ text, count }));
}

export function verifySummary(md, tweets, byId) {
  const ids = [...new Set(md.match(/\b\d{18,20}\b/g) || [])];
  const quotes = [...md.matchAll(/"([^"\n]{12,})"/g)].map((m) => m[1]);
  return {
    ids: ids.length,
    unknownIds: ids.filter((id) => !byId.has(id)),
    quotes: quotes.length,
    unverified: quotes.filter((q) => !tweets.some((t) => norm(t.text).includes(norm(q)))),
  };
}

function main() {
  const args = process.argv.slice(2);
  const opt = (k) => (args.includes(k) ? args[args.indexOf(k) + 1] : null);
  const positional = args.filter((a, i) => !a.startsWith("--") && !(i > 0 && ["--top", "--labels", "--since", "--until"].includes(args[i - 1])));
  const [cleanPath, dir] = positional;
  const topN = Number(opt("--top") ?? 300);
  const store = opt("--labels");
  if (!cleanPath || !dir) {
    console.error("Usage: aggregate.mjs <clean.json> <dir> --labels <labels.jsonl> [--since D] [--until D] [--top N]");
    process.exit(1);
  }
  const tweets = JSON.parse(readFileSync(cleanPath, "utf8"));
  const byId = new Map(tweets.map((t) => [t.id, t]));
  const since = opt("--since"), until = opt("--until");
  const windowed = tweets.filter((t) => { const d = dayOf(t); return (!since || d >= since) && (!until || d < until); });
  if (args.includes("--timeline")) {
    for (const { day, n, top } of timeline(windowed)) {
      console.log(`\n## ${day} (${n})`);
      for (const t of top) console.log(`  [${t.likes}♥ ${t.id}] @${t.author}: ${t.text.replace(/\s+/g, " ").replace(/https?:\S+/g, "").slice(0, 160)}`);
    }
    return;
  }
  const top = new Set([...windowed].sort((a, b) => b.likes - a.likes).slice(0, topN).map((t) => t.id));

  let labels = [];
  const files = readdirSync(dir);
  if (store && existsSync(store)) labels = readLabels(store);
  else for (const f of files.filter((f) => /^labels\d+\.json$/.test(f)).sort()) {
    try { for (const l of JSON.parse(readFileSync(join(dir, f), "utf8"))) labels.push(l); }  // spread overflows the stack past ~100k
    catch (e) { console.log(`bad ${f}: ${e.message}`); }
  }
  const all = labels.length;
  labels = labels.filter((l) => inWindow(l, byId, since, until));
  const labeledIds = new Set(labels.map((l) => l.id));
  const missing = windowed.filter((t) => !labeledIds.has(t.id)).length;
  console.log(`labels: ${labels.length} posts in window (${all} in store, ${missing} window posts without a label)`);

  const pct = (o) => { const n = Object.values(o).reduce((a, b) => a + b, 0); return Object.fromEntries(Object.entries(o).map(([k, v]) => [k, `${v} (${((100 * v) / n).toFixed(1)}%)`])); };
  console.log("sentiment, all:", pct(tally(labels)));
  console.log(`sentiment, top ${topN} by likes:`, pct(tally(labels.filter((l) => top.has(l.id)))));

  const likes = labels.filter((l) => l.about && l.sentiment === "like");
  const interested = likes.filter((l) => l.interest);
  console.log(`likes ${likes.length}, interested-party ${interested.length} (${((100 * interested.length) / (likes.length || 1)).toFixed(0)}%):`, tally(interested.map((l) => ({ sentiment: l.interest }))));
  const dislikes = labels.filter((l) => l.sentiment === "dislike");
  const requests = labels.filter((l) => l.request);
  console.log("\ntopics (about=true)"); console.table(byTopic(labels.filter((l) => l.about && l.topic && l.topic !== "none"), byId));
  console.log("like by feature"); console.table(byFeature(likes, byId));
  console.log("dislike by feature"); console.table(byFeature(dislikes, byId));
  console.log("requests by feature"); console.table(byFeature(requests, byId));
  console.log("top like points"); console.table(topTexts(likes, "point"));
  console.log("top dislike points"); console.table(topTexts(dislikes, "point"));
  console.log("top requests"); console.table(topTexts(requests, "request"));

  for (const f of files.filter((f) => /^summary\d+\.(md|txt)$/.test(f)).sort()) {
    const v = verifySummary(readFileSync(join(dir, f), "utf8"), tweets, byId);
    console.log(`\n${f}: ids ${v.ids} (unknown ${v.unknownIds.length}), quotes ${v.quotes} (unverified ${v.unverified.length})`);
    for (const q of v.unverified) console.log("  ✗", q.slice(0, 90));
  }
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split("/").pop())) main();
