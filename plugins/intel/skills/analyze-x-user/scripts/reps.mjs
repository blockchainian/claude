#!/usr/bin/env node
// ABOUTME: Prints the representative posts a profile.md is written from, one block per doc section, chosen
// ABOUTME: by label (top posts per topic, market posts in date order, every stake, calls, assets, mentions, apps).
//
// Usage: reps.mjs <user-dir> [--per 15] [--apps <archive-slugs>] [--out <file>]
//        <user-dir> holds tweets.jsonl, replies.jsonl and labels.jsonl. Prints plain text (or writes --out);
//        each post is one line: date, engagement, kind/topic/asset/stance/interest, url, text (cut at 200 chars).
import { readFileSync, writeFileSync } from "node:fs";
import { join, basename } from "node:path";
import { mentions } from "./profile-user.mjs";

const engOf = (t) => ["likes", "retweets", "replies", "quotes"].reduce((s, k) => s + (Number(t[k]) || 0), 0);
const byEng = (a, b) => engOf(b) - engOf(a);
const byDate = (a, b) => Date.parse(a.created_at) - Date.parse(b.created_at);
const tally = (items) => { const m = new Map(); for (const x of items) m.set(x, (m.get(x) || 0) + 1); return m; };
const ranked = (map) => [...map.entries()].sort((a, b) => b[1] - a[1]);

// The blocks, from the labeled posts: `per` posts per topic / asset / stake, market posts in date order.
export function sections(posts, labels, { per = 20, apps = [] } = {}) {
  const L = (p) => labels.get(p.id);
  const labeled = posts.filter((p) => L(p));
  const content = labeled.filter((p) => L(p).about);
  const pick = (sel, n = per) => content.filter(sel).sort(byEng).slice(0, n);
  const topics = ranked(tally(content.map((p) => L(p).topic).filter((t) => t && t !== "none"))).slice(0, 10)
    .map(([topic, count]) => ({ topic, count, posts: pick((p) => L(p).topic === topic, per) }));
  const market = content.filter((p) => ["market", "BTC"].includes(L(p).asset) && L(p).stance !== "none").sort(byEng).slice(0, per * 2).sort(byDate);
  const stakes = {};
  for (const [interest] of ranked(tally(content.map((p) => L(p).interest).filter(Boolean)))) stakes[interest] = pick((p) => L(p).interest === interest);
  const calls = pick((p) => L(p).kind === "call", per * 2);
  const promos = pick((p) => L(p).kind === "promo");
  const assets = ranked(tally(content.map((p) => L(p).asset).filter((a) => a && a !== "none" && a !== "market"))).slice(0, 10)
    .map(([asset, count]) => ({ asset, count, posts: pick((p) => L(p).asset === asset, Math.ceil(per / 2)) }));
  const self = posts[0]?.author;
  const handles = ranked(tally(posts.flatMap((p) => mentions(p.text, self)))).slice(0, 10)
    .map(([handle]) => ({ handle, posts: posts.filter((p) => mentions(p.text, self).includes(handle)).sort(byEng).slice(0, 3) }));
  const rx = apps.length ? new RegExp(apps.map((a) => a.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|"), "i") : null;
  const appPosts = rx ? posts.filter((p) => rx.test(p.text || "")).sort(byEng) : [];
  return { topics, market, stakes, calls, promos, assets, mentions: handles, apps: appPosts };
}

const line = (p, l) => `${new Date(p.created_at).toISOString().slice(0, 10)} eng ${engOf(p)} [${l ? [l.kind, l.topic, l.asset, l.stance, l.interest ?? "-"].join("/") : "unlabeled"}] ${p.url || `https://x.com/${p.author}/status/${p.id}`}\n  ${(p.text || "").replace(/\s+/g, " ").slice(0, 200)}`;

export function render(s, labels) {
  const L = (p) => labels.get(p.id);
  const block = (title, ps) => [`## ${title} (${ps.length})`, ...ps.map((p) => line(p, L(p))), ""];
  const out = [];
  for (const t of s.topics) out.push(...block(`topic ${t.topic} — ${t.count} posts`, t.posts));
  out.push(...block("market / BTC, by date", s.market));
  for (const [k, ps] of Object.entries(s.stakes)) out.push(...block(`stake ${k}`, ps));
  out.push(...block("calls", s.calls), ...block("promos", s.promos));
  for (const a of s.assets) out.push(...block(`asset ${a.asset} — ${a.count} posts`, a.posts));
  for (const m of s.mentions) out.push(...block(`@${m.handle}`, m.posts));
  out.push(...block("apps", s.apps));
  return out.join("\n");
}

function readJsonl(path) {
  try { return readFileSync(path, "utf8").split("\n").filter((l) => l.trim()).map((l) => JSON.parse(l)); } catch { return []; }
}

function main() {
  const args = process.argv.slice(2);
  const opt = (k, d = null) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
  const dir = args.find((a, i) => !a.startsWith("--") && !args[i - 1]?.startsWith("--"));
  if (!dir) { console.error("usage: reps.mjs <user-dir> [--per 15] [--apps a,b] [--out <file>]"); process.exit(1); }
  const posts = [...readJsonl(join(dir, "tweets.jsonl")), ...readJsonl(join(dir, "replies.jsonl"))];
  const labels = new Map(readJsonl(join(dir, "labels.jsonl")).map((l) => [l.id, l]));
  if (!labels.size) { console.error(`no labels.jsonl under ${dir}`); process.exit(1); }
  const apps = opt("--apps", "").split(",").filter(Boolean);
  const text = render(sections(posts, labels, { per: Number(opt("--per", "15")), apps }), labels);
  const out = opt("--out");
  if (out) { writeFileSync(out, text); console.error(`-> ${out} (${text.split("\n").length} lines) for @${basename(dir)}`); }
  else process.stdout.write(text);
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split("/").pop())) main();
