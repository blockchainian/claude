// ABOUTME: Posts one video to the secrets-manager TikTok account through TikTok Studio's upload page, in the
// ABOUTME: account's own Camoufox profile, screen-recorded, then finds the posted video's id and logs the post.
//
// Usage:
//   node ${CLAUDE_PLUGIN_ROOT}/skills/upload-tiktok-video/scripts/upload-tiktok-video.mjs <video.mp4> \
//     [--username <name>] [--caption <text>] [--visibility everyone|friends|only-me] [--ai-generated]
//   --username posts as that account of the store (default: its earliest imported active account).
//   --caption is the post's description, hashtags included (default: empty, TikTok then shows nothing).
//   --visibility is who can see the post (default everyone).
//   --ai-generated turns on TikTok's "AI-generated content" label.
//
// The account (the named one, else the store's earliest imported active one) is opened as its
// own profile on its own ISP slot (see tiktok-session.mjs). The window is shown and recorded to
// <CREATE_TIKTOK_DIR>/recordings/<username>-<ts>.mov. A logged-out profile stops the run: log the
// account in again with secrets-manager's `login tiktok`; the store is never written here.
//
// Each post appends a line to <CREATE_TIKTOK_DIR>/posts.jsonl:
//   { at, username, file, caption, visibility, aiGenerated, videoId, url, recording }
// videoId is null when the video had not reached the profile when the run gave up looking.

import { appendFileSync, existsSync, mkdirSync, realpathSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { accountVideos, captureTemplate, dataDir, loadAccount, openBrowser, signedIn, startRecording, stopRecording } from "./tiktok-session.mjs";

const UPLOAD_URL = "https://www.tiktok.com/tiktokstudio/upload?from=webapp";
const UPLOAD_TIMEOUT_MS = 300000; // the file upload, from choosing it to TikTok's "Uploaded"
const POSTED_TIMEOUT_MS = 120000; // from pressing Post to leaving the upload page
const FIND_TIMEOUT_MS = 300000; // how long the posted video may take to show on the profile
const FIND_EVERY_MS = 5000; // between looks at the profile, give or take FIND_JITTER_MS
const FIND_JITTER_MS = 1500;
const CLOCK_SKEW_S = 120; // TikTok's createTime against this machine's clock
// The labels of "Who can see this post", by --visibility.
const VISIBILITY = { everyone: "Everyone", friends: "Friends", "only-me": "Only you" };
const USAGE =
  "Usage: upload-tiktok-video.mjs <video.mp4> [--username <name>] [--caption <text>] [--visibility everyone|friends|only-me] [--ai-generated]";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export function parseArgs(argv) {
  const out = { file: null, username: null, caption: "", visibility: "everyone", aiGenerated: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--caption") out.caption = argv[++i] ?? "";
    else if (a === "--username") out.username = (argv[++i] ?? "").replace(/^@/, "");
    else if (a === "--visibility") {
      out.visibility = argv[++i];
      if (!VISIBILITY[out.visibility]) throw new Error(`--visibility must be one of ${Object.keys(VISIBILITY).join(", ")}`);
    } else if (a === "--ai-generated") out.aiGenerated = true;
    else if (a.startsWith("--")) throw new Error(`Unknown option ${a}`);
    else out.file = a;
  }
  if (!out.file) throw new Error(USAGE);
  return out;
}

// The newest of the account's videos created at or after `sinceS` (unix seconds), or null.
export function findPosted(items, sinceS) {
  return items.filter((v) => v.createTime >= sinceS).sort((a, b) => b.createTime - a.createTime)[0] ?? null;
}

// The wait before the next look at the profile: FIND_EVERY_MS moved by a random amount up to
// FIND_JITTER_MS either way, so the looks do not come on a fixed beat.
export function findDelay(random = Math.random) {
  return Math.round(FIND_EVERY_MS - FIND_JITTER_MS + random() * 2 * FIND_JITTER_MS);
}

// Answer what TikTok Studio lays over the form whenever it gets in the way of the next step, and
// never wait for it otherwise: the "new editing features" tour is closed, and the offer to turn on
// automatic content checks (music copyright and For You eligibility, an account setting) is accepted.
async function handleOverlays(page) {
  const tour = page.getByRole("button", { name: "Got it", exact: true });
  await page.addLocatorHandler(tour, () => tour.click());
  await page.addLocatorHandler(page.getByText("Turn on automatic content checks?"), () =>
    page.getByRole("button", { name: "Turn on", exact: true }).click(),
  );
}

// Replace the description TikTok prefilled (the file name) with `caption`, typed as a person would:
// the field is a Draft.js editor, which ignores a value set from outside.
async function setCaption(page, caption) {
  const editor = page.locator("[data-e2e=caption_container] .public-DraftEditor-content");
  await editor.click();
  await page.keyboard.press("Meta+A");
  await page.keyboard.press("Backspace");
  if (caption) await page.keyboard.type(caption, { delay: 40 });
  await page.keyboard.press("Escape"); // closes a hashtag or mention suggestion list
}

async function setVisibility(page, visibility) {
  const select = page.locator("[data-e2e=video_visibility_container] [role=combobox]");
  if ((await select.innerText()).trim() === VISIBILITY[visibility]) return;
  await select.click();
  await page.getByRole("option", { name: VISIBILITY[visibility] }).or(page.getByText(VISIBILITY[visibility], { exact: true }).last()).first().click();
  const now = (await select.innerText()).trim();
  if (now !== VISIBILITY[visibility]) throw new Error(`visibility is "${now}", not "${VISIBILITY[visibility]}"`);
}

// Turn on the "AI-generated content" switch under "Show more". It turns on at once, with no
// confirmation ("Creator labeled as AI-generated" appears under it).
async function labelAiGenerated(page) {
  const more = page.getByText("Show more", { exact: true });
  if (await more.isVisible().catch(() => false)) await more.click();
  const toggle = page.locator(
    'xpath=//*[normalize-space(text())="AI-generated content"]/ancestor::*[.//input[contains(@class,"Switch__input")]][1]//input[contains(@class,"Switch__input")]',
  );
  if (!(await toggle.isChecked())) await toggle.click({ force: true });
  // The switch reads as checked only once the page has re-rendered (about half a second), so wait
  // for the line it shows when on rather than reading the switch straight after the click.
  await page.getByText("Creator labeled as AI-generated").waitFor({ timeout: 10000 });
  if (!(await toggle.isChecked())) throw new Error("the AI-generated content label did not turn on");
}

// Press Post and wait until TikTok takes the post and leaves the upload page, confirming a "post
// now?" question on the way (asked while its checks are still running).
async function post(page) {
  await page.locator("[data-e2e=post_video_button]").click();
  const deadline = Date.now() + POSTED_TIMEOUT_MS;
  while (Date.now() < deadline) {
    if (!page.url().includes("/upload")) return;
    const now = page.getByRole("button", { name: /^Post now$/ });
    if (await now.isVisible().catch(() => false)) await now.click();
    await sleep(1000);
  }
  throw new Error("still on the upload page after pressing Post");
}

// The posted video, looked for on the account's profile until it shows up or the time runs out.
async function findOnProfile(context, username, sinceS) {
  const page = await context.newPage();
  try {
    const template = captureTemplate(page);
    await page.goto(`https://www.tiktok.com/@${username}`, { waitUntil: "domcontentloaded", timeout: 60000 });
    const base = await template();
    for (const deadline = Date.now() + FIND_TIMEOUT_MS; ; ) {
      const found = findPosted((await accountVideos(page, base, username)).items, sinceS);
      if (found || Date.now() > deadline) return found;
      console.log("  the video is not on the profile yet");
      await sleep(findDelay());
    }
  } finally {
    await page.close().catch(() => {});
  }
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const file = resolve(args.file);
  if (!existsSync(file)) throw new Error(`No such file: ${file}`);
  const account = loadAccount(process.env, args.username);
  if (!account) {
    throw new Error(`No active TikTok account${args.username ? ` @${args.username}` : ""} in the secrets-manager store; run its \`login tiktok\`.`);
  }
  console.log(`posting ${basename(file)} as @${account.username} (ISP slot ${account.slot})`);

  const startedS = Math.floor(Date.now() / 1000) - CLOCK_SKEW_S;
  const { context, page } = await openBrowser({ slot: account.slot, profile: account.profile, headed: true, url: UPLOAD_URL });
  const stamp = new Date().toISOString().replace(/[:.]/g, "-");
  const recording = startRecording(account.profile, join(dataDir(), "recordings", `${account.username}-${stamp}.mov`));
  if (recording) console.log(`  recording to ${recording.path}`);
  const finish = async () => {
    const saved = await stopRecording(recording);
    if (saved) console.log(`  saved recording: ${saved}`);
    await context.close().catch(() => {});
  };
  process.once("SIGINT", () => finish().then(() => process.exit(130)));

  try {
    if (!(await signedIn(context))) {
      throw new Error(`@${account.username}'s profile is logged out; run the secrets-manager skill's \`login tiktok\``);
    }
    await handleOverlays(page);
    const input = page.locator("input[type=file]").first();
    await input.waitFor({ state: "attached", timeout: 60000 });
    await input.setInputFiles(file);
    await page.locator("[data-e2e=upload_status_container]").getByText(/^Uploaded/).waitFor({ timeout: UPLOAD_TIMEOUT_MS });
    console.log("  uploaded");
    await setCaption(page, args.caption);
    await setVisibility(page, args.visibility);
    if (args.aiGenerated) await labelAiGenerated(page);
    console.log(`  caption, ${args.visibility}${args.aiGenerated ? ", AI-generated label" : ""} set`);
    await post(page);
    console.log("  posted");

    const video = await findOnProfile(context, account.username, startedS);
    const url = video ? `https://www.tiktok.com/@${account.username}/video/${video.id}` : null;
    mkdirSync(dataDir(), { recursive: true });
    const record = {
      at: new Date().toISOString(),
      username: account.username,
      file,
      caption: args.caption,
      visibility: args.visibility,
      aiGenerated: args.aiGenerated,
      videoId: video?.id ?? null,
      url,
      recording: recording?.path ?? null,
    };
    appendFileSync(join(dataDir(), "posts.jsonl"), JSON.stringify(record) + "\n");
    console.log(video ? `posted ${url}` : "posted, but the video has not reached the profile yet; track-tiktok-stats will list it once it has");
  } finally {
    await finish();
  }
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) await main();
