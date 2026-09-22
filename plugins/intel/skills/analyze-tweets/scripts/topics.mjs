#!/usr/bin/env node
// ABOUTME: Counts hot topics over the whole clean corpus from a label→regex table (hits, authors,
// ABOUTME: likes, peak day) and prints each day's top posts by likes as the event timeline.
//
// Usage: topics.mjs <clean.json> --topics <topics.json> [--per-day 3]
//   topics.json: {"<中文标签>": "<regex, case-insensitive>", ...}
import { readFileSync } from "node:fs";
import { dayOf } from "./clean.mjs";

export function countTopics(tweets, topics) {
  const rows = [];
  for (const [label, pattern] of Object.entries(topics)) {
    const re = new RegExp(pattern, "i");
    const hits = tweets.filter((t) => re.test(t.text));
    const byDay = {};
    for (const t of hits) byDay[dayOf(t)] = (byDay[dayOf(t)] || 0) + 1;
    const peak = Object.entries(byDay).sort((a, b) => b[1] - a[1])[0];
    rows.push({
      topic: label,
      n: hits.length,
      authors: new Set(hits.map((t) => t.author)).size,
      likes: hits.reduce((s, t) => s + (t.likes || 0), 0),
      peakDay: peak?.[0] ?? null,
      peakN: peak?.[1] ?? 0,
    });
  }
  return rows.sort((a, b) => b.n - a.n);
}

export function timeline(tweets, perDay = 3) {
  const byDay = {};
  for (const t of tweets) (byDay[dayOf(t)] = byDay[dayOf(t)] || []).push(t);
  return Object.keys(byDay).sort().map((d) => ({
    day: d,
    n: byDay[d].length,
    top: byDay[d].sort((a, b) => b.likes - a.likes).slice(0, perDay),
  }));
}

function main() {
  const args = process.argv.slice(2);
  const input = args.find((a) => !a.startsWith("--"));
  const topicsPath = args[args.indexOf("--topics") + 1];
  const perDay = args.includes("--per-day") ? Number(args[args.indexOf("--per-day") + 1]) : 3;
  if (!input || !topicsPath) {
    console.error("Usage: topics.mjs <clean.json> --topics <topics.json> [--per-day N]");
    process.exit(1);
  }
  const tweets = JSON.parse(readFileSync(input, "utf8"));
  const topics = JSON.parse(readFileSync(topicsPath, "utf8"));
  console.table(countTopics(tweets, topics));
  for (const { day, n, top } of timeline(tweets, perDay)) {
    console.log(`\n## ${day} (${n})`);
    for (const t of top) {
      const text = t.text.replace(/\s+/g, " ").replace(/https?:\S+/g, "").slice(0, 160);
      console.log(`  [${t.likes}♥ ${t.id}] @${t.author}: ${text}`);
    }
  }
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split("/").pop())) main();
