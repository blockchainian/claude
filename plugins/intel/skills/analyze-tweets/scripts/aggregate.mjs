#!/usr/bin/env node
// ABOUTME: Merges the labelers' output: sentiment over all posts and over the top-liked, like/dislike
// ABOUTME: counts per topic, interested-party share of praise, and id/quote checks for every summary.
//
// Usage: aggregate.mjs <clean.json> <dir-with-labels*.json-and-summary*.md> [--top 300]
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

export const norm = (s) =>
  s.replace(/\s+/g, " ").replace(/[’‘]/g, "'").replace(/[“”]/g, '"').replace(/&amp;/g, "&")
    .toLowerCase().replace(/[.!?]+$/, "").trim();

const INTERESTED = /callout|payout|paid out|\$[0-9]|rewards?|referral|sent from my/i;

export function tally(rows) {
  const o = {};
  for (const r of rows) o[r.sentiment] = (o[r.sentiment] || 0) + 1;
  return o;
}

export function byTopic(rows, byId) {
  const n = {}, authors = {};
  for (const r of rows) {
    n[r.topic] = (n[r.topic] || 0) + 1;
    (authors[r.topic] = authors[r.topic] || new Set()).add(byId.get(r.id)?.author);
  }
  return Object.entries(n).sort((a, b) => b[1] - a[1]).map(([topic, count]) => ({ topic, count, authors: authors[topic].size }));
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
  const [cleanPath, dir] = args.filter((a) => !a.startsWith("--"));
  const topN = args.includes("--top") ? Number(args[args.indexOf("--top") + 1]) : 300;
  if (!cleanPath || !dir) {
    console.error("Usage: aggregate.mjs <clean.json> <dir> [--top N]");
    process.exit(1);
  }
  const tweets = JSON.parse(readFileSync(cleanPath, "utf8"));
  const byId = new Map(tweets.map((t) => [t.id, t]));
  const top = new Set([...tweets].sort((a, b) => b.likes - a.likes).slice(0, topN).map((t) => t.id));

  const labels = [];
  const files = readdirSync(dir);
  for (const f of files.filter((f) => /^labels\d+\.json$/.test(f)).sort()) {
    try { labels.push(...JSON.parse(readFileSync(join(dir, f), "utf8"))); }
    catch (e) { console.log(`bad ${f}: ${e.message}`); }
  }
  const expected = files.filter((f) => /^chunk\d+\.json$/.test(f)).length;
  const got = files.filter((f) => /^labels\d+\.json$/.test(f)).length;
  console.log(`labels: ${labels.length} posts from ${got}/${expected} chunks`);

  const pct = (o) => { const n = Object.values(o).reduce((a, b) => a + b, 0); return Object.fromEntries(Object.entries(o).map(([k, v]) => [k, `${v} (${((100 * v) / n).toFixed(1)}%)`])); };
  console.log("sentiment, all:", pct(tally(labels)));
  console.log(`sentiment, top ${topN} by likes:`, pct(tally(labels.filter((l) => top.has(l.id)))));

  const likes = labels.filter((l) => l.sentiment === "like");
  const interested = likes.filter((l) => l.topic === "callout-rewards-kols" || INTERESTED.test(byId.get(l.id)?.text || ""));
  console.log(`likes ${likes.length}, interested-party ${interested.length} (${((100 * interested.length) / (likes.length || 1)).toFixed(0)}%)`);
  console.log("\nlike by topic"); console.table(byTopic(likes, byId));
  console.log("dislike by topic"); console.table(byTopic(labels.filter((l) => l.sentiment === "dislike"), byId));
  console.log("topics, about=true"); console.table(byTopic(labels.filter((l) => l.about), byId).slice(0, 20));

  for (const f of files.filter((f) => /^summary\d+\.(md|txt)$/.test(f)).sort()) {
    const v = verifySummary(readFileSync(join(dir, f), "utf8"), tweets, byId);
    console.log(`\n${f}: ids ${v.ids} (unknown ${v.unknownIds.length}), quotes ${v.quotes} (unverified ${v.unverified.length})`);
    for (const q of v.unverified) console.log("  ✗", q.slice(0, 90));
  }
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split("/").pop())) main();
