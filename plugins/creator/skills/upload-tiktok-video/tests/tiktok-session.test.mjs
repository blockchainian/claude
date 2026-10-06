import { spawnSync } from 'node:child_process';
// ABOUTME: Tests the pure parts of tiktok-session.mjs: the account pick, the store lookup, proxies,
// ABOUTME: the request template, the browser pid lookup, stopping a recording and the data directory.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { test } from "node:test";

import { accountDir, apiUrl, browserFlag, browserPrefs, dataDir, displayName, findPid, ispProxyAt, loadAccount, pickAccount, proxyDict, recordingPath, stopRecording, templateFrom } from "../scripts/tiktok-session.mjs";

const row = (username, status, isp_slot, created_at) => ({ username, status, isp_slot, created_at });

test("pickAccount takes the earliest imported active account that has a slot", () => {
  const rows = [
    row("late", "active", 2, "2026-10-04T19:41:10Z"),
    row("banned", "restricted", 7, "2026-10-01T00:00:00Z"),
    row("noslot", "active", null, "2026-10-01T00:00:00Z"),
    row("early", "active", 10, "2026-10-02T03:58:32Z"),
  ];
  assert.equal(pickAccount(rows).username, "early");
});

test("pickAccount breaks a created_at tie by username", () => {
  const rows = [row("bob", "active", 2, "2026-10-04T19:41:10Z"), row("amy", "active", 1, "2026-10-04T19:41:10Z")];
  assert.equal(pickAccount(rows).username, "amy");
});

test("pickAccount takes the named account when it is active and has a slot", () => {
  const rows = [row("early", "active", 10, "2026-10-02T00:00:00Z"), row("Named", "active", 3, "2026-10-04T00:00:00Z"), row("gone", "expired", 4, "2026-10-01T00:00:00Z")];
  assert.equal(pickAccount(rows, "named").username, "Named");
  assert.equal(pickAccount(rows, "gone"), null);
  assert.equal(pickAccount(rows, "nobody"), null);
});

test("pickAccount takes a named account in any status when asked, still only with a slot", () => {
  const rows = [row("gone", "restricted", 4, "2026-10-01T00:00:00Z"), row("noslot", "restricted", null, "2026-10-01T00:00:00Z"), row("early", "active", 1, "2026-09-01T00:00:00Z")];
  assert.equal(pickAccount(rows, "gone", { anyStatus: true }).username, "gone");
  assert.equal(pickAccount(rows, "noslot", { anyStatus: true }), null);
  assert.equal(pickAccount(rows, null, { anyStatus: true }).username, "early"); // the default account is always an active one
});

test("pickAccount is null without an active account", () => {
  assert.equal(pickAccount([row("x", "expired", 1, "2026-10-01T00:00:00Z")]), null);
});

test("loadAccount reads the secrets-manager store, names the profile and gives the status", () => {
  const state = mkdtempSync(join(tmpdir(), "creator-store-"));
  const db = new DatabaseSync(join(state, "secrets.sqlite"));
  db.exec("CREATE TABLE tiktok (username TEXT, password TEXT, isp_slot INTEGER, status TEXT, created_at TEXT, updated_at TEXT)");
  const insert = db.prepare("INSERT INTO tiktok VALUES (?, 'pw', ?, ?, ?, ?)");
  insert.run("second", 3, "active", "2026-10-04T00:00:00Z", "2026-10-04T00:00:00Z");
  insert.run("first", 9, "active", "2026-10-02T00:00:00Z", "2026-10-04T00:00:00Z");
  db.close();
  assert.deepEqual(loadAccount({ SECRETS_DATA_DIR: state }), {
    username: "first",
    status: "active",
    slot: 9,
    profile: join(state, "profiles", "first"),
  });
});

test("loadAccount takes a named account", () => {
  const state = mkdtempSync(join(tmpdir(), "creator-store-"));
  const db = new DatabaseSync(join(state, "secrets.sqlite"));
  db.exec("CREATE TABLE tiktok (username TEXT, password TEXT, isp_slot INTEGER, status TEXT, created_at TEXT, updated_at TEXT)");
  const insert = db.prepare("INSERT INTO tiktok VALUES (?, 'pw', ?, 'active', ?, ?)");
  insert.run("first", 9, "2026-10-02T00:00:00Z", "2026-10-04T00:00:00Z");
  insert.run("other", 3, "2026-10-04T00:00:00Z", "2026-10-04T00:00:00Z");
  db.close();
  assert.equal(loadAccount({ SECRETS_DATA_DIR: state }, "other").slot, 3);
  const again = new DatabaseSync(join(state, "secrets.sqlite"));
  again.exec("UPDATE tiktok SET status = 'restricted' WHERE username = 'other'");
  again.close();
  assert.equal(loadAccount({ SECRETS_DATA_DIR: state }, "other"), null);
  assert.equal(loadAccount({ SECRETS_DATA_DIR: state }, "other", { anyStatus: true }).status, "restricted");
});

test("loadAccount is null when there is no store", () => {
  assert.equal(loadAccount({ SECRETS_DATA_DIR: join(tmpdir(), "no-such-store") }), null);
});

test("loadAccount is null when the readable store has no usable account", (t) => {
  const state = mkdtempSync(join(tmpdir(), "creator-store-"));
  t.after(() => rmSync(state, { recursive: true, force: true }));
  const db = new DatabaseSync(join(state, "secrets.sqlite"));
  db.exec("CREATE TABLE tiktok (username TEXT, status TEXT, isp_slot INTEGER, created_at TEXT)");
  db.exec("INSERT INTO tiktok VALUES ('inactive', 'expired', 1, '2026-10-04'), ('no-slot', 'active', NULL, '2026-10-04')");
  db.close();
  assert.equal(loadAccount({ SECRETS_DATA_DIR: state }), null);
});

test("loadAccount preserves database open errors", (t) => {
  const state = mkdtempSync(join(tmpdir(), "creator-store-"));
  t.after(() => rmSync(state, { recursive: true, force: true }));
  mkdirSync(join(state, "secrets.sqlite"));
  assert.throws(() => loadAccount({ SECRETS_DATA_DIR: state }), {
    code: "ERR_SQLITE_ERROR",
    message: /unable to open database file|disk I\/O error/,
  });
});

test("loadAccount preserves corrupt database errors", (t) => {
  const state = mkdtempSync(join(tmpdir(), "creator-store-"));
  t.after(() => rmSync(state, { recursive: true, force: true }));
  writeFileSync(join(state, "secrets.sqlite"), "not a SQLite database");
  assert.throws(() => loadAccount({ SECRETS_DATA_DIR: state }), {
    code: "ERR_SQLITE_ERROR",
    message: "file is not a database",
  });
});

test("loadAccount preserves schema errors", (t) => {
  const state = mkdtempSync(join(tmpdir(), "creator-store-"));
  t.after(() => rmSync(state, { recursive: true, force: true }));
  const db = new DatabaseSync(join(state, "secrets.sqlite"));
  db.close();
  assert.throws(() => loadAccount({ SECRETS_DATA_DIR: state }), {
    code: "ERR_SQLITE_ERROR",
    message: "no such table: tiktok",
  });
});

test("ispProxyAt adds the slot to the base port", () => {
  assert.equal(ispProxyAt("http://u:p@isp.example:10000", 3), "http://u:p@isp.example:10003");
});

test("proxyDict splits the credentials out of the url", () => {
  assert.deepEqual(proxyDict("http://u%40x:p@isp.example:10003"), { server: "http://isp.example:10003", username: "u@x", password: "p" });
});

test("templateFrom keeps an API request's params without its signatures", () => {
  const url = "https://www.tiktok.com/api/user/detail/?aid=1988&device_id=42&uniqueId=a&msToken=t&X-Bogus=b&X-Gnarly=g";
  assert.deepEqual(templateFrom(url), { aid: "1988", device_id: "42", uniqueId: "a" });
  assert.equal(templateFrom("https://www.tiktok.com/api/user/detail/?aid=1988"), null);
  assert.equal(templateFrom("https://www.tiktok.com/foryou?device_id=42"), null);
});

test("apiUrl lays the call's params over the template", () => {
  assert.equal(apiUrl({ aid: "1988", count: "10" }, "post/item_list/", { count: 35 }), "https://www.tiktok.com/api/post/item_list/?aid=1988&count=35");
});

test("findPid picks the Camoufox main process this process launched, with or without a profile", () => {
  const ps = [
    "  101    1 /Applications/Camoufox.app/Contents/MacOS/camoufox -profile /state/profiles/other",
    "  202  303 /Applications/Camoufox.app/Contents/MacOS/camoufox -contentproc plugin-container /state/profiles/me",
    "  303   50 /Applications/Camoufox.app/Contents/MacOS/camoufox -profile /tmp/playwright_firefoxdev_profile-x",
    "  404   50 /usr/bin/swift recordWindows.swift 303 /x.mov",
  ].join("\n");
  assert.equal(findPid(50, ps), 303);
  assert.equal(findPid(51, ps), null);
});

test("displayName is the secrets-manager's BROWSER_DISPLAY, or empty for the main display", () => {
  assert.equal(displayName({ BROWSER_DISPLAY: "SAMSUNG" }), "SAMSUNG");
  assert.equal(displayName({}), "");
});

test("browserPrefs turns the sound back on only when asked", () => {
  assert.deepEqual(browserPrefs(false), {});
  assert.deepEqual(browserPrefs(true), { "media.volume_scale": "1.0" });
});

test("browserFlag reads --headed and --with-sound into the options", () => {
  const out = { headed: false, withSound: false };
  assert.equal(browserFlag("--headed", out), true);
  assert.equal(browserFlag("--with-sound", out), true);
  assert.equal(browserFlag("--other", out), false);
  assert.deepEqual(out, { headed: true, withSound: true });
});

test("recordingPath names a run's recording by its skill and time, under the account", () => {
  const at = new Date("2026-10-05T01:20:23.996Z");
  assert.equal(recordingPath("me", "upload", at, { CREATOR_DATA_DIR: "/x" }), "/x/tiktok/me/recordings/upload-2026-10-05T01-20-23-996Z.mov");
});

test("dataDir defaults under ~/.local/share and follows CREATOR_DATA_DIR", () => {
  assert.equal(dataDir({}), join(homedir(), ".local", "share", "creator", "tiktok"));
  assert.equal(dataDir({ CREATOR_DATA_DIR: "/x" }), "/x/tiktok");
});

test("accountDir keeps each account's data in its own directory under dataDir", () => {
  assert.equal(accountDir("me", { CREATOR_DATA_DIR: "/x" }), "/x/tiktok/me");
  assert.equal(accountDir("me", {}), join(dataDir({}), "me"));
});

test("stopRecording waits for the recorder to exit and leaves no timer holding the process open", async () => {
  const child = spawn(process.execPath, ["-e", 'process.on("SIGINT", () => process.exit(0)); console.log("ready"); setInterval(() => {}, 1000)']);
  await new Promise((r) => child.stdout.once("data", r));
  assert.equal(await stopRecording({ child, path: "/x.mov" }), "/x.mov");
  assert.equal(child.exitCode, 0);
  assert.ok(!process.getActiveResourcesInfo().includes("Timeout"), "a timer is still pending");
});

test('Creator reads its own dotenv while retaining the shared account-state directory', () => {
  const home = mkdtempSync(join(tmpdir(), 'creator-config-'));
  mkdirSync(join(home, '.config', 'creator'), { recursive: true });
  mkdirSync(join(home, '.config', 'secrets-manager'), { recursive: true });
  writeFileSync(join(home, '.config', 'creator', '.env'), 'BROWSER_DISPLAY=own-display\nCREATOR_DATA_DIR=/tmp/creator-fixture\n');
  writeFileSync(join(home, '.config', 'secrets-manager', '.env'), 'BROWSER_DISPLAY=wrong-display\n');
  const moduleUrl = new URL('../scripts/tiktok-session.mjs', import.meta.url).href;
  const code = `const {displayName,dataDir,storeDir}=await import(${JSON.stringify(moduleUrl)}); if(displayName()!=='own-display'||dataDir()!=='/tmp/creator-fixture/tiktok') throw Error('wrong config source'); console.log(storeDir());`;
  const env = { ...process.env, HOME: home };
  for (const key of ['BROWSER_DISPLAY','CREATOR_DATA_DIR','SECRETS_DATA_DIR']) delete env[key];
  const result = spawnSync(process.execPath, [...process.execArgv, '--input-type=module', '-e', code], { env, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.trim(), join(home, '.config', 'secrets-manager'));
});
