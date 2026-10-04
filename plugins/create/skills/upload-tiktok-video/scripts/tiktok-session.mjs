// ABOUTME: Opens TikTok in Camoufox, either as the secrets-manager account's own profile or anonymously,
// ABOUTME: on the account's ISP slot, calls TikTok's web API signed by its own page, and screen-records headed runs.
//
// The account is the secrets-manager store's earliest imported `active` TikTok row. It is opened
// exactly as `login tiktok` opened it: the same profile directory and stored fingerprint, the same ISP
// slot (a fixed IP), English locale, timezone from the exit IP. To TikTok that is the same device the
// account signed in on. The store is only read; logging in and account status belong to secrets-manager.
//
// Config (SECRETS_MANAGER_STATE_PATH/.env, the secrets-manager's own, loaded automatically):
//   ISP_PROXY_URL               the ISP pool's base url; slot n is the base port + n.
//   SECRETS_MANAGER_STATE_PATH  where the store and profiles are (default ~/.config/secrets-manager).
//   CREATE_TIKTOK_DIR           where posts, stats and recordings go (default ~/.local/share/create/tiktok).

import { spawn, execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync } from "node:fs";
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

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export const storeDir = (env = process.env) => env.SECRETS_MANAGER_STATE_PATH || join(homedir(), ".config", "secrets-manager");

export const dataDir = (env = process.env) => env.CREATE_TIKTOK_DIR || join(homedir(), ".local", "share", "create", "tiktok");

// The secrets-manager's .env carries the ISP pool; a value already in the environment wins.
const envFile = join(storeDir(), ".env");
if (existsSync(envFile)) process.loadEnvFile(envFile);

// The account to act as: the earliest imported active row that has an ISP slot (its login ran),
// ties broken by username. Null when there is none.
export function pickAccount(rows) {
  const usable = rows.filter((r) => r.status === "active" && r.isp_slot != null);
  usable.sort((a, b) => a.created_at.localeCompare(b.created_at) || a.username.localeCompare(b.username));
  return usable[0] ?? null;
}

// The picked account with the profile directory it logged in with, or null without a store or account.
export function loadAccount(env = process.env) {
  let rows;
  try {
    const db = new DatabaseSync(join(storeDir(env), "secrets.sqlite"), { readOnly: true });
    rows = db.prepare("SELECT username, status, isp_slot, created_at FROM tiktok").all();
    db.close();
  } catch {
    return null;
  }
  const row = pickAccount(rows);
  return row ? { username: row.username, slot: row.isp_slot, profile: join(storeDir(env), "profiles", row.username) } : null;
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

// The pid of the Camoufox main process running `profileDir`, from `ps -Ao pid=,args=` output.
export function findPid(profileDir, ps = execFileSync("ps", ["-Ao", "pid=,args="], { encoding: "utf8" })) {
  for (const line of ps.split("\n")) {
    if (line.includes(profileDir) && line.includes("MacOS/camoufox") && !line.includes("plugin-container") && !line.includes("gpu-helper")) {
      const pid = Number.parseInt(line.trim().split(/\s+/, 1)[0], 10);
      if (Number.isInteger(pid)) return pid;
    }
  }
  return null;
}

// Camoufox on the account's ISP slot at `url`. With `profile` it is the account's own browser
// profile and fingerprint, signed in or not; without, an anonymous browser. `headed` shows the
// window. Returns { context, page, template }: `template()` resolves to the base of signed API calls.
export async function openBrowser({ slot, profile = null, headed = false, url }) {
  if (!process.env.ISP_PROXY_URL) throw new Error(`No ISP_PROXY_URL in ${envFile}.`);
  const { Camoufox } = await import("camoufox-js");
  const context = await Camoufox({
    headless: !headed,
    geoip: true, // timezone and locale follow the exit IP
    locale: "en-US", // as `login tiktok` opens it, so the page's labels are English
    proxy: proxyDict(ispProxyAt(process.env.ISP_PROXY_URL, slot)),
    main_world_eval: true, // API calls run in the page's own world, where TikTok's script signs fetch()
    ...(profile && {
      user_data_dir: profile,
      fingerprint: JSON.parse(readFileSync(join(profile, "fingerprint.json"), "utf8")),
      i_know_what_im_doing: true, // the fingerprint is the one the profile logged in with
    }),
  });
  try {
    const page = context.pages?.()[0] ?? (await context.newPage());
    const template = captureTemplate(page);
    await page.goto(url, { waitUntil: "domcontentloaded", timeout: 60000 });
    return { context, page, template };
  } catch (e) {
    await context.close().catch(() => {});
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

// Start recording the windows of the Camoufox running `profile` to `path` (.mov, cursor shown).
// macOS only and best-effort: returns a handle for stopRecording, or null.
export function startRecording(profile, path) {
  const pid = process.platform === "darwin" ? findPid(profile) : null;
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
  const exited = new Promise((r) => child.once("exit", r));
  child.kill("SIGINT");
  await Promise.race([exited, sleep(timeoutMs)]);
  return handle.path;
}
