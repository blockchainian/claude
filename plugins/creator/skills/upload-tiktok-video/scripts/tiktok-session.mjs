// ABOUTME: Opens TikTok in Camoufox, either as the secrets-manager account's own profile or anonymously,
// ABOUTME: on the account's ISP slot, calls TikTok's web API signed by its own page, and screen-records headed runs.
//
// The account is the secrets-manager store's earliest imported `active` TikTok row. It is opened
// exactly as `login tiktok` opened it: the same profile directory and stored fingerprint, the same ISP
// slot (a fixed IP), English locale, timezone from the exit IP. To TikTok that is the same device the
// account signed in on. The store is only read; logging in and account status belong to secrets-manager.
//
// Config (~/.config/creator/.env, loaded automatically):
//   ISP_PROXY_URL               the ISP pool's base url; slot n is the base port + n.
//   BROWSER_DISPLAY            the display a headed window goes on, any part of its name (else the main one).
//   SECRETS_DATA_DIR  where the store and profiles are (default ~/.config/secrets-manager).
//   CREATOR_DATA_DIR           where posts, stats and recordings go, one directory per account
//                               (root default ~/.local/share/creator; tiktok/ is appended).

import { execFile, execFileSync, spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, statSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { fileURLToPath } from "node:url";

const API = "https://www.tiktok.com/api/";
// The signatures TikTok's page script adds to a request; a new request gets its own.
const SIGN_KEYS = ["X-Bogus", "X-Gnarly", "X-Dynosaur", "msToken"];
const SESSION_COOKIE = "sessionid"; // the cookie of a signed-in tiktok.com browser
const TEMPLATE_WAIT_MS = 30000;
const REQUEST_TIMEOUT_MS = 30000;
const RECORD_SCRIPT = fileURLToPath(new URL("./recordWindows.swift", import.meta.url));
const MOVE_SCRIPT = fileURLToPath(new URL("./moveWindows.swift", import.meta.url));

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export const storeDir = (env = process.env) => env.SECRETS_DATA_DIR || join(homedir(), ".config", "secrets-manager");

export const dataDir = (env = process.env) => join(env.CREATOR_DATA_DIR || join(homedir(), ".local", "share", "creator"), "tiktok");

// Where one account's posts, stats and recordings go.
export const accountDir = (username, env = process.env) => join(dataDir(env), username);

// Creator's .env carries the ISP pool; a value already in the environment wins.
const envFile = join(homedir(), ".config", "creator", ".env");
if (existsSync(envFile)) process.loadEnvFile(envFile);

// The account to act as: the one named `username` (any case), else the earliest imported, ties
// broken by username; either way an active row with an ISP slot (its login ran). `anyStatus` lets a
// named account be in any status (to look at a banned one); the default account is always active.
// Null when there is none.
export function pickAccount(rows, username = null, { anyStatus = false } = {}) {
  const usable = rows.filter(
    (r) =>
      (r.status === "active" || (anyStatus && username)) &&
      r.isp_slot != null &&
      (!username || r.username.toLowerCase() === username.toLowerCase()),
  );
  usable.sort((a, b) => a.created_at.localeCompare(b.created_at) || a.username.localeCompare(b.username));
  return usable[0] ?? null;
}

// The picked account with the profile directory it logged in with, or null without a store or account.
export function loadAccount(env = process.env, username = null, options = {}) {
  const path = join(storeDir(env), "secrets.sqlite");
  try {
    statSync(path);
  } catch (error) {
    if (error.code === "ENOENT") return null;
    throw error;
  }
  const db = new DatabaseSync(path, { readOnly: true });
  let rows;
  try {
    rows = db.prepare("SELECT username, status, isp_slot, created_at FROM tiktok").all();
  } finally {
    db.close();
  }
  const row = pickAccount(rows, username, options);
  return row ? { username: row.username, status: row.status, slot: row.isp_slot, profile: join(storeDir(env), "profiles", row.username) } : null;
}

// Slot `slot` (1-based) of the ISP pool: the same endpoint on the base port + slot, one fixed IP each.
export function ispProxyAt(baseUrl, slot) {
  const u = new URL(baseUrl);
  u.port = String(Number(u.port) + slot);
  return u.toString().replace(/\/$/, "");
}

export function proxyDict(proxyUrl) {
  const u = new URL(proxyUrl);
  const proxy = { server: `${u.protocol}//${u.host}` };
  if (u.username) proxy.username = decodeURIComponent(u.username);
  if (u.password) proxy.password = decodeURIComponent(u.password);
  return proxy;
}

// The query params of a request the page itself sent, without its signatures, or null when the
// url is not an API call carrying the page's device id.
export function templateFrom(requestUrl) {
  const url = new URL(requestUrl);
  if (!url.pathname.startsWith("/api/") || !url.searchParams.has("device_id")) return null;
  return Object.fromEntries([...url.searchParams].filter(([k]) => !SIGN_KEYS.includes(k)));
}

export function apiUrl(base, path, params) {
  return `${API}${path}?${new URLSearchParams({ ...base, ...params })}`;
}

// The pid of the Camoufox main process that `parentPid` launched (Playwright starts it directly), from
// `ps -Ao pid=,ppid=,args=` output. Its own content and GPU processes are its children, not ours.
export function findPid(parentPid = process.pid, ps = execFileSync("ps", ["-Ao", "pid=,ppid=,args="], { encoding: "utf8" })) {
  for (const line of ps.split("\n")) {
    const [pid, ppid, ...args] = line.trim().split(/\s+/);
    if (Number(ppid) === parentPid && args.join(" ").includes("MacOS/camoufox")) return Number(pid);
  }
  return null;
}

// The display a headed window goes on: BROWSER_DISPLAY in Creator's .env, any part of
// the display's name in any case (e.g. SAMSUNG), shared with its headed logins; empty means the main display.
export const displayName = (env = process.env) => env.BROWSER_DISPLAY ?? "";

// Move the windows of the Camoufox `pid` that are off the display onto it, through the Accessibility
// API. macOS only and best-effort: resolves either way, never rejects.
export function moveToDisplay(pid, name = displayName()) {
  if (process.platform !== "darwin" || !pid) return Promise.resolve();
  return new Promise((resolve) => execFile("swift", [MOVE_SCRIPT, String(pid), "40", name], { timeout: 60000 }, () => resolve()));
}

// The Firefox prefs a launch adds: Playwright mutes the browser (media.volume_scale 0), and
// `withSound` turns it back up, for a person watching a headed run.
export function browserPrefs(withSound) {
  return withSound ? { "media.volume_scale": "1.0" } : {};
}

// Read the browser options every creator script takes into `out`: --headed shows the window and
// screen-records it (for debugging a flow TikTok's page changes broke), --with-sound unmutes it.
// Returns whether `arg` was one of them.
export function browserFlag(arg, out) {
  if (arg === "--headed") out.headed = true;
  else if (arg === "--with-sound") out.withSound = true;
  else return false;
  return true;
}

// Refuse --with-sound without --headed: a hidden browser has no one to listen.
export function checkBrowserFlags(out) {
  if (out.withSound && !out.headed) throw new Error("--with-sound needs --headed");
}

// Where a headed run of `kind` (upload, stats, sounds) started `at` records to, under the account.
export function recordingPath(username, kind, at = new Date(), env = process.env) {
  return join(accountDir(username, env), "recordings", `${kind}-${at.toISOString().replace(/[:.]/g, "-")}.mov`);
}

// Camoufox on the account's ISP slot at `url`. With `profile` it is the account's own browser
// profile and fingerprint, signed in or not; without, an anonymous browser. `headed` shows the
// window on the BROWSER_DISPLAY display (moved there, as are the windows it opens later) and, given
// `recordTo`, screen-records it; `withSound` unmutes it. Returns
// { context, page, template, close }: `template()` resolves to the base of signed API calls, and
// `close()` saves the recording and closes the browser.
export async function openBrowser({ slot, profile = null, headed = false, withSound = false, recordTo = null, url }) {
  if (!process.env.ISP_PROXY_URL) throw new Error(`No ISP_PROXY_URL in ${envFile}.`);
  const { Camoufox } = await import("camoufox-js");
  const context = await Camoufox({
    headless: !headed,
    geoip: true, // timezone and locale follow the exit IP
    locale: "en-US", // as `login tiktok` opens it, so the page's labels are English
    proxy: proxyDict(ispProxyAt(process.env.ISP_PROXY_URL, slot)),
    main_world_eval: true, // API calls run in the page's own world, where TikTok's script signs fetch()
    firefox_user_prefs: browserPrefs(withSound),
    ...(profile && {
      user_data_dir: profile,
      fingerprint: JSON.parse(readFileSync(join(profile, "fingerprint.json"), "utf8")),
      i_know_what_im_doing: true, // the fingerprint is the one the profile logged in with
    }),
  });
  const recording = headed && recordTo ? startRecording(recordTo) : null;
  if (recording) console.log(`  recording to ${recording.path}`);
  const close = async () => {
    const saved = await stopRecording(recording);
    if (saved) console.log(`  saved recording: ${saved}`);
    await context.close().catch(() => {});
  };
  try {
    const page = context.pages?.()[0] ?? (await context.newPage());
    const template = captureTemplate(page);
    await page.goto(url, { waitUntil: "domcontentloaded", timeout: 60000 });
    if (headed) {
      const pid = findPid();
      await moveToDisplay(pid);
      context.on("page", () => moveToDisplay(pid));
    }
    return { context, page, template, close };
  } catch (e) {
    await close();
    throw e;
  }
}

// Whether the browser holds a signed-in tiktok.com session.
export async function signedIn(context) {
  const cookies = await context.cookies("https://www.tiktok.com");
  return cookies.some((c) => c.name === SESSION_COOKIE && c.value);
}

// The query params of the first API request `page` sends from now on: the base every signed call
// is built on. Call before navigating, await after.
export function captureTemplate(page) {
  let base = null;
  page.on("request", (request) => {
    base ??= templateFrom(request.url());
  });
  return async () => {
    for (let waited = 0; !base && waited < TEMPLATE_WAIT_MS; waited += 500) await sleep(500);
    if (!base) throw new Error("the page sent no API request");
    return base;
  };
}

// One API call made inside the page's main world, where TikTok's script signs it. An empty or
// non-JSON body is an error.
export async function callApi(page, base, path, params) {
  const url = apiUrl(base, path, params);
  const text = await page.evaluate(
    `mw:fetch(${JSON.stringify(url)}, { credentials: "include", signal: AbortSignal.timeout(${REQUEST_TIMEOUT_MS}) }).then((r) => r.text())`,
  );
  if (!text) throw new Error(`${path}: empty response`);
  try {
    return JSON.parse(text);
  } catch {
    throw new Error(`${path}: non-JSON response`);
  }
}

// An account's secUid and the first page of its videos (TikTok's full items, newest first).
export async function accountVideos(page, base, username) {
  const user = await callApi(page, base, "user/detail/", { uniqueId: username, secUid: "" });
  const secUid = user.userInfo?.user?.secUid;
  if (!secUid) throw new Error(`TikTok does not know @${username}`);
  const list = await callApi(page, base, "post/item_list/", { secUid, count: 35, cursor: 0 });
  return { user: user.userInfo, items: list.itemList ?? [] };
}

// Start recording the windows of the Camoufox this process launched to `path` (.mov, cursor shown).
// macOS only and best-effort: returns a handle for stopRecording, or null.
export function startRecording(path) {
  const pid = process.platform === "darwin" ? findPid() : null;
  if (!pid) return null;
  try {
    mkdirSync(join(path, ".."), { recursive: true });
    const child = spawn("swift", [RECORD_SCRIPT, String(pid), path], { stdio: "ignore" });
    child.on("error", () => {});
    return { child, path };
  } catch {
    return null;
  }
}

// Stop a recording: the recorder finalizes the .mov on SIGINT and then exits, so wait for the exit
// (a kill before it leaves an unplayable file). Returns the path, or null when nothing was recorded.
export async function stopRecording(handle, timeoutMs = 15000) {
  const child = handle?.child;
  if (!child || child.exitCode !== null || child.signalCode !== null) return null;
  let timer;
  const exited = new Promise((r) => child.once("exit", r));
  child.kill("SIGINT");
  // The timeout is cleared once the recorder exits: left armed, it would hold the process open.
  await Promise.race([exited, new Promise((r) => (timer = setTimeout(r, timeoutMs)))]);
  clearTimeout(timer);
  return handle.path;
}
