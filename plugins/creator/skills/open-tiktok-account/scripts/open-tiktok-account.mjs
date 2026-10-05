// ABOUTME: Opens a secrets-manager TikTok account's own Camoufox profile in a shown window, signed in as
// ABOUTME: that account, for a person to look around (private posts, Studio analytics), and waits until it is closed.
//
// Usage:
//   node ${CLAUDE_PLUGIN_ROOT}/skills/open-tiktok-account/scripts/open-tiktok-account.mjs [--username <name>] [--url <url>] [--with-sound]
//   --username opens that account of the store (default: the account upload-tiktok-video posts as, the
//     store's earliest imported active TikTok account).
//   --url is the page to open (default TikTok Studio's home).
//   --with-sound unmutes the browser (Playwright mutes it), to hear the videos.
//
// The window is always shown, so there is no --headed; nothing is recorded, a person is watching.
// The run ends when the window is closed (on macOS that closes the last tab, not the browser, so the
// browser is closed then too). Nothing else may have the profile open at the same time.

import { realpathSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { loadAccount, openBrowser, signedIn } from "../../upload-tiktok-video/scripts/tiktok-session.mjs";

const STUDIO_URL = "https://www.tiktok.com/tiktokstudio";

// Resolves once the person is done: the browser closed, or its last tab (the window) closed.
export function untilClosed(context) {
  return new Promise((resolve) => {
    context.on("close", resolve);
    const watch = (page) => page.on("close", () => context.pages().length || resolve());
    context.pages().forEach(watch);
    context.on("page", watch);
  });
}

export function parseArgs(argv) {
  const out = { username: null, url: STUDIO_URL, withSound: false };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--username") out.username = (argv[++i] ?? "").replace(/^@/, "");
    else if (argv[i] === "--url") {
      out.url = argv[++i] ?? "";
      if (!/^https:\/\/(www\.)?tiktok\.com\//.test(out.url)) throw new Error("--url must be a https://www.tiktok.com/ page");
    } else if (argv[i] === "--with-sound") out.withSound = true;
    else throw new Error(`Unknown option ${argv[i]}`);
  }
  return out;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const account = loadAccount(process.env, args.username);
  if (!account) {
    throw new Error(`No active TikTok account${args.username ? ` @${args.username}` : ""} in the secrets-manager store; run its \`login tiktok\`.`);
  }
  const { context, close } = await openBrowser({ slot: account.slot, profile: account.profile, headed: true, withSound: args.withSound, url: args.url });
  process.once("SIGINT", () => close().then(() => process.exit(130)));
  if (!(await signedIn(context))) console.log(`  @${account.username}'s profile is logged out; run the secrets-manager skill's \`login tiktok\``);
  console.log(`opened @${account.username} at ${args.url}${args.withSound ? ", with sound" : ""}; close the window to end`);
  await untilClosed(context);
  await close();
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) await main();
