// ABOUTME: Bulk-fetch follower + lifetime-tweet counts for high-volume authors via api.fxtwitter.com
// ABOUTME: (no auth), so the bot filter can apply the followers-vs-views / followers-vs-volume rules.
import { stateDir, loadEnvFile } from "../../fetch-x-mentions/scripts/env.mjs";
import { join } from "node:path";
loadEnvFile();
import { readFileSync, writeFileSync, existsSync } from "node:fs";

const app = process.argv[2];
const MINPOSTS = 50, CONC = 8;
const tweetsPath = join(stateDir(), "x", app, "tweets.jsonl");
const outPath = join(stateDir(), "x", app, "followers.json");

const n = {};
for (const line of readFileSync(tweetsPath, "utf8").split("\n")) {
  if (!line.trim()) continue;
  let d; try { d = JSON.parse(line); } catch { continue; }
  const a = d.author; if (a) n[a] = (n[a] || 0) + 1;
}
const handles = Object.keys(n).filter((a) => n[a] >= MINPOSTS);
const out = existsSync(outPath) ? JSON.parse(readFileSync(outPath, "utf8")) : {};
const todo = handles.filter((h) => !(h in out));
console.log(`${app}: ${handles.length} handles >=${MINPOSTS} posts, ${todo.length} to fetch`);

async function one(h) {
  for (let attempt = 0; attempt < 3; attempt++) {
    try {
      const r = await fetch(`https://api.fxtwitter.com/${encodeURIComponent(h)}`, {
        headers: { "User-Agent": "Mozilla/5.0" }, signal: AbortSignal.timeout(15000),
      });
      if (r.status === 404) return { followers: null, tweets: null, status: 404 }; // suspended/deleted
      if (!r.ok) { await new Promise((s) => setTimeout(s, 1500)); continue; }
      const j = await r.json();
      const u = j.user || {};
      return { followers: u.followers ?? null, tweets: u.tweets ?? null, following: u.following ?? null, status: 200 };
    } catch { await new Promise((s) => setTimeout(s, 1500)); }
  }
  return { followers: null, tweets: null, status: "err" };
}

let i = 0, done = 0;
async function worker() {
  while (i < todo.length) {
    const h = todo[i++];
    out[h] = await one(h);
    done++;
    if (done % 50 === 0) { writeFileSync(outPath, JSON.stringify(out)); console.log(`  ${done}/${todo.length}`); }
  }
}
await Promise.all(Array.from({ length: CONC }, worker));
writeFileSync(outPath, JSON.stringify(out));
const susp = Object.values(out).filter((v) => v.status === 404).length;
console.log(`${app}: done, ${Object.keys(out).length} cached, ${susp} suspended/404`);
