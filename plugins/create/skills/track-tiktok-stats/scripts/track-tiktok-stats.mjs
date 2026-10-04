// ABOUTME: Records the current play, like, comment, share and save counts of a TikTok account's videos,
// ABOUTME: read anonymously from its profile page through TikTok's own signed web API, one row per video per run.
//
// Usage:
//   node ${CLAUDE_PLUGIN_ROOT}/skills/track-tiktok-stats/scripts/track-tiktok-stats.mjs [--username <name>]
//   --username names the account to read; default the account upload-tiktok-video posts as (the
//   secrets-manager store's earliest imported active TikTok account).
//
// The read is anonymous: a fresh Camoufox browser, not the account's profile, on the account's ISP
// slot. TikTok gives an anonymous viewer the first page of a profile (about 35 videos), newest first.
// Each run appends one row per video to <CREATE_TIKTOK_DIR>/stats.jsonl:
//   { at, username, videoId, createTime, playCount, diggCount, commentCount, shareCount, collectCount }
// Run it on a schedule to build the time series.

import { appendFileSync, mkdirSync, realpathSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { accountVideos, dataDir, loadAccount, openBrowser } from "../../upload-tiktok-video/scripts/tiktok-session.mjs";

const START_URL = "https://www.tiktok.com/explore";
const COUNTERS = ["playCount", "diggCount", "commentCount", "shareCount", "collectCount"];

export function parseArgs(argv) {
  const out = { username: null };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--username") out.username = argv[++i].replace(/^@/, "");
    else throw new Error(`Unknown option ${argv[i]}`);
  }
  return out;
}

// One row per video: its counters at `at`, 0 for a counter TikTok left out.
export function statsRows(username, items, at) {
  return items.map((v) => ({
    at,
    username,
    videoId: v.id,
    createTime: v.createTime,
    ...Object.fromEntries(COUNTERS.map((k) => [k, Number(v.stats?.[k] ?? 0)])),
  }));
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const account = loadAccount();
  if (!account) throw new Error("No active TikTok account in the secrets-manager store; run its `login tiktok`.");
  const username = args.username ?? account.username;
  const { context, page, template } = await openBrowser({ slot: account.slot, url: START_URL });
  try {
    const { items } = await accountVideos(page, await template(), username);
    const rows = statsRows(username, items, new Date().toISOString());
    const out = join(dataDir(), "stats.jsonl");
    mkdirSync(dataDir(), { recursive: true });
    if (rows.length) appendFileSync(out, rows.map((r) => JSON.stringify(r)).join("\n") + "\n");
    for (const r of rows) {
      console.log(`${r.videoId}  plays ${r.playCount}  likes ${r.diggCount}  comments ${r.commentCount}  shares ${r.shareCount}  saves ${r.collectCount}`);
    }
    console.log(`@${username}: ${rows.length} video(s) -> ${out}`);
  } finally {
    await context.close().catch(() => {});
  }
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) await main();
