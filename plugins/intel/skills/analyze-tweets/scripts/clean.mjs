#!/usr/bin/env node
// ABOUTME: Cleans a fetch-x-mentions tweets.jsonl: drops bot alert templates, mass-tag posts,
// ABOUTME: near-duplicates and stubs, then prints the corpus facts the reception doc opens with.
//
// Usage: clean.mjs <tweets.jsonl> --out <clean.json> [--since YYYY-MM-DD] [--until YYYY-MM-DD] [--bot-pattern <regex>]
import { readFileSync, writeFileSync } from "node:fs";

// Token-alert bot templates and DM-spam; extend per corpus with --bot-pattern.
const BOT_DEFAULT =
  "Route: |Venue: |Launch: |MIGRATION|Migration |CTO SIGNAL|CTO ALERT|WALLET FLOW CHECK|Quick Buy|Quick Swap|CHECK EVENTS|Volume Alert|dm us";
const MAX_TAGS = 6;
const MIN_LEN = 8;

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

export function clean(tweets, botPattern = BOT_DEFAULT, since = null, until = null) {
  const botRe = new RegExp(botPattern, "i");
  const seen = new Set();
  const out = [];
  const dropped = { outOfRange: 0, bot: 0, massTag: 0, dupe: 0, short: 0 };
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
  return { clean: out, dropped };
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
    console.error("Usage: clean.mjs <tweets.jsonl> --out <clean.json> [--since YYYY-MM-DD] [--until YYYY-MM-DD] [--bot-pattern <regex>]");
    process.exit(1);
  }
  const raw = readLog(input);
  const { clean: cleaned, dropped } = clean(raw, pat, opt("--since"), opt("--until"));
  writeFileSync(out, JSON.stringify(cleaned));
  console.log(JSON.stringify({ ...facts(raw, cleaned), dropped }, null, 1));
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split("/").pop())) main();
