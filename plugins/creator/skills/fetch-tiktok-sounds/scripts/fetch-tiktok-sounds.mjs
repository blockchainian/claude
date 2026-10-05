// ABOUTME: Lists the hottest sounds of TikTok's commercial (royalty-free) music library, as TikTok Studio's
// ABOUTME: "Royalty-free sounds" page shows them, with the ids upload-tiktok-video's --sound takes.
//
// Usage:
//   node ${CLAUDE_PLUGIN_ROOT}/skills/fetch-tiktok-sounds/scripts/fetch-tiktok-sounds.mjs [--username <name>] [--count <n>]
//   --username reads the list as that account of the store (default: the account upload-tiktok-video
//     posts as, the store's earliest imported active TikTok account).
//   --count is how many sounds to list, hottest first (default 20).
//
// The list is Studio's own, so it needs a signed-in account: the account's Camoufox profile is opened
// headless on its ISP slot, and the list endpoint is called with fetch() inside the Studio page.
// Prints one line per sound: id, title, author, length, and how many posts use it.

import { realpathSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { loadAccount, openBrowser, signedIn } from "../../upload-tiktok-video/scripts/tiktok-session.mjs";

const LIBRARY_URL = "https://www.tiktok.com/tiktokstudio/sound-library";
const LIST_URL = "https://www.tiktok.com/tiktok/v1/creator/music/unlimited/list/?aid=1988";
const PAGE_SIZE = 20;

export function parseArgs(argv) {
  const out = { username: null, count: 20 };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--username") out.username = (argv[++i] ?? "").replace(/^@/, "");
    else if (argv[i] === "--count") {
      out.count = Number(argv[++i]);
      if (!Number.isInteger(out.count) || out.count < 1) throw new Error("--count must be a whole number of sounds, 1 or more");
    } else throw new Error(`Unknown option ${argv[i]}`);
  }
  return out;
}

// The list requests covering `count` sounds: the page's own request (hot first, the "Sounds" tab),
// one page at a time.
export function pageBodies(count) {
  return Array.from({ length: Math.ceil(count / PAGE_SIZE) }, (_, i) => ({ page_num: i + 1, page_size: PAGE_SIZE, source: 1, order_by: 1, music_type: 1 }));
}

// One row per sound. `id_str` is the id: the numeric `id` has lost digits to float precision.
export function soundRows(list) {
  return list.map((m) => ({ id: m.id_str, title: m.title, author: m.author, seconds: m.duration, posts: m.user_count }));
}

const minutes = (s) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const account = loadAccount(process.env, args.username);
  if (!account) {
    throw new Error(`No active TikTok account${args.username ? ` @${args.username}` : ""} in the secrets-manager store; run its \`login tiktok\`.`);
  }
  const { context, page } = await openBrowser({ slot: account.slot, profile: account.profile, url: LIBRARY_URL });
  try {
    if (!(await signedIn(context))) {
      throw new Error(`@${account.username}'s profile is logged out; run the secrets-manager skill's \`login tiktok\``);
    }
    const rows = [];
    for (const body of pageBodies(args.count)) {
      const text = await page.evaluate(
        `mw:fetch(${JSON.stringify(LIST_URL)}, { method: "POST", credentials: "include", headers: { "content-type": "application/json" }, body: ${JSON.stringify(JSON.stringify(body))} }).then((r) => r.text())`,
      );
      const res = JSON.parse(text);
      if (res.status_code !== 0) throw new Error(`the sound list answered ${res.status_code} ${res.status_msg ?? ""}`);
      rows.push(...soundRows(res.data?.music_list ?? []));
      if (!res.data?.has_more) break;
    }
    for (const r of rows.slice(0, args.count)) {
      console.log(`${r.id}  ${r.title} — ${r.author}  ${minutes(r.seconds)}  ${r.posts} posts`);
    }
  } finally {
    await context.close().catch(() => {});
  }
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) await main();
