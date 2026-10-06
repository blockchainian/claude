// ABOUTME: Unit tests for the secrets-manager CLI: the vendor X line decoder, argv parsing, and the
// ABOUTME: browser-free commands (import, get, set-status, list, export, login/verify skip paths).

import { test, beforeEach, afterEach } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, symlinkSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { parseXVendorLine, parseTiktokLine, parseFile, parseCli, main, needsRefresh, loginSkipReason, googleBrowserOpts } from "../scripts/cli.mjs";
import { parseLine } from "../scripts/credentials.mjs";
import * as config from "../scripts/config.mjs";
import * as store from "../scripts/store.mjs";

test("parseFile skips blanks and # comments, collects per-line errors", () => {
  const text = ["# header", "", "a@x.com:pw", "broken", "  ", "f@x.com:pw2:S:"].join("\n");
  const { accounts, errors } = parseFile(text, parseLine);
  assert.deepEqual(accounts.map((a) => a.email), ["a@x.com", "f@x.com"]);
  assert.equal(errors.length, 1);
  assert.match(errors[0], /line 4/);
});

// The vendor files use two shapes (6 and 8 fields) and put the auth-token/TOTP columns in either
// order; the decoder keys on field shape, not position. Values below are fake.
const AUTH40 = "0123456789abcdef0123456789abcdef01234567";
const TOTP16 = "ABCDEFGHIJKLMNOP";

test("parseXVendorLine reads the 6-field shape", () => {
  const row = parseXVendorLine(`bob:pw:bob@mail.com:emailpw:${AUTH40}:${TOTP16}`);
  assert.deepEqual(row, { username: "bob", password: "pw", email: "bob@mail.com", email_password: "emailpw", totp_secret: TOTP16, auth_token: AUTH40 });
});

test("parseXVendorLine reads both 8-field column orders", () => {
  const totpFirst = parseXVendorLine(`bob:pw:bob@mail.com:emailpw:longtoken:some-uuid:${TOTP16}:${AUTH40}`);
  const authFirst = parseXVendorLine(`bob:pw:bob@mail.com:emailpw:longtoken:some-uuid:${AUTH40}:${TOTP16}`);
  assert.equal(totpFirst.totp_secret, TOTP16);
  assert.equal(authFirst.totp_secret, TOTP16);
  assert.equal(totpFirst.email, "bob@mail.com");
});

test("parseXVendorLine rejects lines with no TOTP secret or too few fields", () => {
  assert.throws(() => parseXVendorLine(`bob:pw:bob@mail.com:emailpw:${AUTH40}`), /TOTP/);
  assert.throws(() => parseXVendorLine("bob:pw:bob@mail.com:emailpw"), /5 fields/);
});

test("parseTiktokLine takes the first four fields and ignores the profile url", () => {
  const row = parseTiktokLine("bob1:pw:bob@mail.com:emailpw:https://www.tiktok.com/@bob1?lang=en");
  assert.deepEqual(row, { username: "bob1", password: "pw", email: "bob@mail.com", email_password: "emailpw" });
  assert.deepEqual(parseTiktokLine("bob1:pw"), { username: "bob1", password: "pw", email: null, email_password: null });
});

test("parseTiktokLine does not take a profile url for the email pair", () => {
  const row = parseTiktokLine("bob1:pw:https://www.tiktok.com/@bob1");
  assert.deepEqual(row, { username: "bob1", password: "pw", email: null, email_password: null });
});

test("parseTiktokLine rejects a line with no password", () => {
  assert.throws(() => parseTiktokLine("bob1"), /password/);
  assert.throws(() => parseTiktokLine("bob1::bob@mail.com"), /password/);
});

// --- argv parsing -------------------------------------------------------------------

test("a Google sign-in runs headed unless --headless asks otherwise", () => {
  assert.deepEqual(googleBrowserOpts(parseCli(["login", "google"]).opts), { headed: true, rotate: undefined });
  assert.deepEqual(googleBrowserOpts(parseCli(["setup-2fa", "--headless", "--rotate-proxy"]).opts), { headed: false, rotate: true });
});

test("parseCli reads repeated --select and the flags", () => {
  const { command, opts } = parseCli(["login", "alpha", "--select", "a@x.com", "--select", "b@x.com", "--headed", "--all", "--limit", "2", "--rotate-proxy"]);
  assert.equal(command, "login");
  assert.deepEqual(opts.positional, ["alpha"]);
  assert.deepEqual(opts.select, ["a@x.com", "b@x.com"]);
  assert.equal(opts.headed, true);
  assert.equal(opts.all, true);
  assert.equal(opts.limit, 2);
  assert.equal(opts["rotate-proxy"], true);
  const beta = parseCli(["login", "beta", "--by-email", "--mint-app-password"]).opts;
  assert.equal(beta["by-email"], true);
  assert.equal(beta["mint-app-password"], true);
  assert.equal(parseCli(["list"]).opts.limit, undefined);
});

test("parseCli reads --concurrency and defaults it to 1", () => {
  assert.equal(parseCli(["login", "google", "--concurrency", "5"]).opts.concurrency, 5);
  assert.equal(parseCli(["login", "google"]).opts.concurrency, 1);
});

test("parseCli rejects the removed per-challenge login flags", () => {
  // The phone step is always driven (HeroSMS) and a headed run is what lets a person clear a
  // reCAPTCHA — there is no --sms or --assist flag anymore.
  assert.throws(() => parseCli(["login", "google", "--sms"]), /Unknown option|Usage/);
  assert.throws(() => parseCli(["login", "google", "--assist"]), /Unknown option|Usage/);
});

test("parseCli keeps import files as positionals after the target", () => {
  assert.deepEqual(parseCli(["import", "x", "a.txt", "b.txt"]).opts.positional, ["x", "a.txt", "b.txt"]);
});

test("parseCli reads the sms verb and its flags", () => {
  const { command, opts } = parseCli(["sms", "number", "--country", "41", "--max-price", "0.06", "--yes"]);
  assert.equal(command, "sms");
  assert.deepEqual(opts.positional, ["number"]);
  assert.equal(opts.country, "41");
  assert.equal(opts["max-price"], "0.06");
  assert.equal(opts.yes, true);
  assert.throws(() => parseCli(["sms"]), /Usage/); // needs a verb
  assert.throws(() => parseCli(["sms", "balance", "prices"]), /Usage/); // one verb only
});

test("parseCli reads setup-2fa (no positional) with its flags", () => {
  const { command, opts } = parseCli(["setup-2fa", "--select", "a@x.com", "--all", "--headed"]);
  assert.equal(command, "setup-2fa");
  assert.deepEqual(opts.positional, []);
  assert.deepEqual(opts.select, ["a@x.com"]);
  assert.equal(opts.all, true);
  assert.equal(opts.headed, true);
  assert.throws(() => parseCli(["setup-2fa", "google"]), /Usage/); // takes no positional target
});

test("parseCli rejects an unknown command, a wrong positional count and an unknown flag", () => {
  assert.throws(() => parseCli(["frobnicate"]), /Usage/);
  assert.throws(() => parseCli(["get"]), /Usage/);
  assert.throws(() => parseCli(["get", "alpha", "a@x.com"]), /Usage/); // the id goes in --select
  assert.throws(() => parseCli(["set-status", "alpha"]), /Usage/);
  assert.throws(() => parseCli(["set-status", "alpha", "a@x.com", "expired"]), /Usage/);
  assert.throws(() => parseCli(["login"]), /Usage/);
  assert.throws(() => parseCli(["login", "x", "--user", "bob"]));
});

// --- commands against a temp state dir ------------------------------------------

let base;
let db;
const io = () => {
  const out = [];
  const err = [];
  return { log: (s) => out.push(s), error: (s) => err.push(s), out, err, text: () => out.join("\n"), errText: () => err.join("\n") };
};

beforeEach(() => {
  process.env.SECRETS_MANAGER_ADAPTERS = fileURLToPath(new URL("./fixtures/adapters.mjs", import.meta.url));
  base = mkdtempSync(join(tmpdir(), "sm-cli-"));
  process.env.SECRETS_MANAGER_STATE_PATH = base;
  db = store.openDb(config.dbPath());
});
afterEach(() => {
  db.close();
  delete process.env.SECRETS_MANAGER_STATE_PATH;
  delete process.env.SECRETS_MANAGER_ADAPTERS;
});

const writeGoogleFile = (text) => {
  mkdirSync(config.credentialsDir("google"), { recursive: true });
  writeFileSync(join(config.credentialsDir("google"), "a.txt"), text);
};

test("needsRefresh is true for a missing or expired session only", () => {
  store.saveSession(db, "keep", "a@x.com", [], {});
  store.saveSession(db, "stale", "a@x.com", [], {});
  store.setSessionStatus(db, "stale", "a@x.com", store.STATUS_EXPIRED);
  store.saveSession(db, "banned", "a@x.com", [], {});
  store.setSessionStatus(db, "banned", "a@x.com", store.STATUS_RESTRICTED);
  assert.equal(needsRefresh(db, "keep", "a@x.com"), false);
  assert.equal(needsRefresh(db, "stale", "a@x.com"), true);
  assert.equal(needsRefresh(db, "banned", "a@x.com"), false);
  assert.equal(needsRefresh(db, "fresh", "a@x.com"), true);
});

test("import google upserts the credential dir into the google table", async () => {
  writeGoogleFile("a@x.com:pw:SECRET:\nb@x.com:pw2\n");
  const o = io();
  assert.equal(await main(["import", "google"], o), 0);
  assert.ok(o.text().includes("imported 2 google account(s)"));
  assert.equal(store.getAccount(db, "a@x.com").totp_secret, "SECRET");
  assert.equal(store.listAccounts(db).length, 2);
});

test("import x reads a named file and reports skipped lines", async () => {
  const file = join(base, "x.txt");
  writeFileSync(file, `bob:pw:bob@mail.com:epw:${AUTH40}:${TOTP16}\nbad line\n`);
  const o = io();
  assert.equal(await main(["import", "x", file], o), 0);
  assert.ok(o.text().includes("imported 1 x account(s)"));
  assert.ok(o.text().includes("skipped line 2"));
  assert.equal(store.getPendingX(db, { force: true })[0].username, "bob");
});

test("import tiktok reads the tiktok dir and a re-import keeps the stored session", async () => {
  mkdirSync(config.credentialsDir("tiktok"), { recursive: true });
  const file = join(config.credentialsDir("tiktok"), "a.txt");
  writeFileSync(file, "bob1:pw:bob@mail.com:epw:https://www.tiktok.com/@bob1?lang=en\n");
  const o = io();
  assert.equal(await main(["import", "tiktok"], o), 0);
  assert.ok(o.text().includes("imported 1 tiktok account(s)"));
  store.saveTiktokLogin(db, "bob1", { cookies: [{ name: "sessionid", value: "S" }], ispSlot: 10 });
  writeFileSync(file, "bob1:pw2:bob@mail.com:epw\n");
  assert.equal(await main(["import", "tiktok"], io()), 0);
  const row = store.getTiktok(db, "bob1");
  assert.equal(row.password, "pw2");
  assert.equal(row.status, store.STATUS_ACTIVE);
  assert.equal(row.isp_slot, 10);
});

test("login tiktok with nothing pending opens no browser", async () => {
  store.upsertTiktok(db, { username: "bob1", password: "pw" });
  store.saveTiktokLogin(db, "bob1", { cookies: [], ispSlot: 10 });
  const o = io();
  assert.equal(await main(["login", "tiktok"], o), 0);
  assert.ok(o.text().includes("no TikTok accounts need login"));
});

test("get and set-status reach the tiktok row by username", async () => {
  store.upsertTiktok(db, { username: "bob1", password: "pw" });
  assert.equal(await main(["set-status", "tiktok", "expired", "--select", "bob1"], io()), 0);
  const o = io();
  assert.equal(await main(["get", "tiktok", "--select", "bob1"], o), 0);
  assert.ok(o.text().includes('"status": "expired"'));
  const missing = io();
  assert.equal(await main(["set-status", "tiktok", "expired", "--select", "nobody"], missing), 1);
  assert.ok(missing.errText().includes("no TikTok account"));
});

test("import rejects an unknown target and an empty dir", async () => {
  const o = io();
  assert.equal(await main(["import", "reddit"], o), 1);
  assert.ok(o.errText().includes("import target must be one of google, x, tiktok"));
  assert.equal(await main(["import", "x"], o), 1);
  assert.ok(o.errText().includes("no files given"));
});

test("list on an empty store, then with an account and its apps", async () => {
  const o = io();
  assert.equal(await main(["list"], o), 0);
  assert.ok(o.text().includes("no accounts yet"));
  store.upsertAccount(db, "a@x.com", "pw", "S", null);
  store.saveSession(db, "alpha", "a@x.com", [], {});
  const t = io();
  assert.equal(await main(["list"], t), 0);
  assert.ok(t.text().includes("a@x.com"));
  assert.ok(t.text().includes("alpha=active"));
  const j = io();
  await main(["list", "--json"], j);
  assert.deepEqual(JSON.parse(j.text())[0].apps, { alpha: "active" });
});

test("get of a missing session returns 1 and reports on stderr", async () => {
  const o = io();
  assert.equal(await main(["get", "alpha", "--select", "a@x.com"], o), 1);
  assert.ok(o.errText().includes("no alpha session for a@x.com"));
});

test("set-status and get round-trip; get x reads the x row", async () => {
  store.saveSession(db, "alpha", "a@x.com", [{ name: "c" }], {});
  assert.equal(await main(["set-status", "alpha", "expired", "--select", "a@x.com"], io()), 0);
  const o = io();
  assert.equal(await main(["get", "alpha", "--select", "a@x.com"], o), 0);
  assert.ok(o.text().includes('"status": "expired"'));
  store.upsertX(db, { username: "bob", password: "p", email: "b@x.com", totp_secret: "s" });
  assert.equal(await main(["set-status", "x", "escalated", "--select", "bob"], io()), 0);
  const x = io();
  assert.equal(await main(["get", "x", "--select", "bob"], x), 0);
  assert.ok(x.text().includes('"status": "escalated"'));
});

test("get and set-status refuse an unregistered app instead of creating a table", async () => {
  const o = io();
  assert.equal(await main(["get", "alphao", "--select", "a@x.com"], o), 1);
  assert.equal(await main(["set-status", "alphao", "expired", "--select", "a@x.com"], o), 1);
  assert.ok(o.errText().includes("app must be one of alpha, beta, x, tiktok"));
  assert.deepEqual(store.listApps(db), []);
});

test("get and set-status need --select, and take several accounts", async () => {
  const o = io();
  assert.equal(await main(["get", "alpha"], o), 1);
  assert.equal(await main(["set-status", "alpha", "expired"], o), 1);
  assert.ok(o.errText().includes("get needs --select"));
  assert.ok(o.errText().includes("set-status needs --select"));
  store.saveSession(db, "alpha", "a@x.com", [], {});
  store.saveSession(db, "alpha", "b@x.com", [], {});
  assert.equal(await main(["set-status", "alpha", "restricted", "--select", "a@x.com", "--select", "b@x.com"], io()), 0);
  const g = io();
  assert.equal(await main(["get", "alpha", "--select", "a@x.com", "--select", "b@x.com"], g), 0);
  assert.equal(g.text().split('"status": "restricted"').length - 1, 2);
});

test("get and set-status report a missing account, still handle the others, and return 1", async () => {
  store.saveSession(db, "alpha", "a@x.com", [], {});
  const s = io();
  assert.equal(await main(["set-status", "alpha", "expired", "--select", "nobody@x.com", "--select", "a@x.com"], s), 1);
  assert.ok(s.errText().includes("has no alpha session"));
  const g = io();
  assert.equal(await main(["get", "alpha", "--select", "nobody@x.com", "--select", "a@x.com"], g), 1);
  assert.ok(g.errText().includes("no alpha session for nobody@x.com"));
  assert.ok(g.text().includes('"status": "expired"'));
});

test("set-status rejects an unknown status and a missing session", async () => {
  const o = io();
  assert.equal(await main(["set-status", "alpha", "sleeping", "--select", "a@x.com"], o), 1);
  assert.ok(o.errText().includes("status must be one of"));
  assert.equal(await main(["set-status", "alpha", "expired", "--select", "a@x.com"], o), 1);
  assert.ok(o.errText().includes("has no alpha session"));
});

test("export prints JSONL credentials only for active sessions and missing on stderr", async () => {
  for (const e of ["a@x.com", "b@x.com", "c@x.com", "d@x.com", "e@x.com"]) store.upsertAccount(db, e, "pw", null, null);
  store.saveSession(db, "beta", "a@x.com", [{ name: "auth-refresh-token", value: "RA" }], []);
  store.saveSession(db, "beta", "b@x.com", [], []);
  store.saveSession(db, "beta", "c@x.com", [{ name: "auth-refresh-token", value: "RC" }], []);
  store.setSessionStatus(db, "beta", "c@x.com", store.STATUS_EXPIRED);
  store.saveSession(db, "beta", "e@x.com", [{ name: "auth-refresh-token", value: "RE" }], []);
  const o = io();
  assert.equal(await main(["export", "beta"], o), 0);
  assert.deepEqual(o.out.map(JSON.parse), [
    { app: "beta", email: "a@x.com", refresh_token: "RA" },
    { app: "beta", email: "e@x.com", refresh_token: "RE" },
  ]);
  assert.deepEqual(o.err, ["b@x.com\tmissing"]);
  const w = io();
  assert.equal(await main(["export", "beta", "--select", "a@x.com", "--select", "c@x.com"], w), 0);
  assert.deepEqual(w.out, ['{"app":"beta","email":"a@x.com","refresh_token":"RA"}']);
  assert.deepEqual(w.err, []);
  const empty = io();
  assert.equal(await main(["export", "beta", "--select", "d@x.com"], empty), 0);
  assert.deepEqual(empty.out, []);
  assert.deepEqual(empty.err, []);
  const g = io();
  assert.equal(await main(["export", "gamma"], g), 1);
  assert.ok(g.errText().includes("unknown adapter gamma; loaded: alpha, beta"));
  assert.deepEqual(g.out, []);
});

test("export supports arbitrary credential fields and fails without the hook", async () => {
  const path = join(base, "adapter.mjs");
  writeFileSync(path, `export default () => [{
    name: 'custom', domain: 'custom.example', startUrl: 'https://custom.example/',
    entryTexts: ['Login'], signIn: async () => {}, ready: async () => true,
    credentials: () => ({api_key: 'key', access_token: 'token'}),
  }, {
    name: 'no_hook', domain: 'custom.example', startUrl: 'https://custom.example/',
    entryTexts: ['Login'], signIn: async () => {}, ready: async () => true,
  }];`);
  process.env.SECRETS_MANAGER_ADAPTERS = path;
  store.upsertAccount(db, "a@x.com", "pw", null, null);
  store.saveSession(db, "custom", "a@x.com", [], []);
  const o = io();
  assert.equal(await main(["export", "custom"], o), 0);
  assert.deepEqual(o.out.map(JSON.parse), [{app: "custom", email: "a@x.com", api_key: "key", access_token: "token"}]);
  assert.deepEqual(o.err, []);
  const missing = io();
  assert.equal(await main(["export", "no_hook"], missing), 1);
  assert.deepEqual(missing.out, []);
  assert.deepEqual(missing.err, ["no_hook has no credentials hook"]);
});

test("parseCli rejects removed --only, --out and export-env", () => {
  for (const args of [
    ["login", "alpha", "--only", "a@x.com"],
    ["export", "alpha", "--out", "tokens.env"],
    ["export-env", "alpha"],
  ]) assert.throws(() => parseCli(args), /Unknown option|Usage/);
});

test("loginSkipReason retries escalated for login google, skips it for apps, always skips restricted", () => {
  assert.equal(loginSkipReason(store.STATUS_RESTRICTED, "google"), "account status restricted");
  assert.equal(loginSkipReason(store.STATUS_RESTRICTED, "alpha"), "account status restricted");
  assert.equal(loginSkipReason(store.STATUS_ESCALATED, "google"), null); // login google retries so a headed run can clear it
  assert.equal(loginSkipReason(store.STATUS_ESCALATED, "alpha"), "account status escalated");
  assert.equal(loginSkipReason(store.STATUS_ACTIVE, "google"), null);
});

test("login <app> skips a restricted and an escalated account and an up-to-date session without a browser", async () => {
  writeGoogleFile("a@x.com:pw:SECRET:\nb@x.com:pw:SECRET:\nc@x.com:pw:SECRET:\n");
  store.upsertAccount(db, "a@x.com", "pw", "SECRET", null);
  store.setAccountStatus(db, "a@x.com", store.STATUS_RESTRICTED);
  store.upsertAccount(db, "c@x.com", "pw", "SECRET", null);
  store.setAccountStatus(db, "c@x.com", store.STATUS_ESCALATED);
  store.saveSession(db, "alpha", "b@x.com", [], []);
  const o = io();
  assert.equal(await main(["login", "alpha"], o), 0);
  assert.ok(o.text().includes("skip a@x.com: account status restricted"));
  assert.ok(o.text().includes("skip c@x.com: account status escalated"));
  assert.ok(o.text().includes("b@x.com: alpha up to date, skipping"));
});

test("login with no credential files, an unknown target, or --by-email off beta fails cleanly", async () => {
  const o = io();
  assert.equal(await main(["login", "alpha"], o), 1);
  assert.ok(o.errText().includes("no credential files"));
  assert.equal(await main(["login", "reddit"], o), 1);
  assert.ok(o.errText().includes("login target must be one of google, x, tiktok, alpha, beta"));
  assert.equal(await main(["login", "alpha", "--by-email"], o), 1);
  assert.ok(o.errText().includes("alpha has no byEmail hook"));
});

test("login x skips an empty queue and verify x is a builtin check", async () => {
  const o = io();
  assert.equal(await main(["login", "x"], o), 0);
  assert.ok(o.text().includes("no X accounts need login"));
  assert.equal(await main(["verify", "x"], o), 0);
  assert.ok(o.text().includes("no X active accounts to check"));
  assert.ok(!o.errText().includes("unknown adapter"));
});

test("verify alpha with no active accounts returns 0; verify rejects other targets", async () => {
  const o = io();
  assert.equal(await main(["verify", "alpha"], o), 0);
  assert.ok(o.text().includes("no alpha active accounts to check"));
  assert.equal(await main(["verify", "gamma"], o), 1);
  assert.ok(o.errText().includes("unknown adapter gamma; loaded: alpha, beta"));
});

// The repo exposes the skill through a symlinked directory too; Node resolves the entry module to
// its real path, so the "run as main" check must compare real paths or the CLI silently does nothing.
test("cli runs main when invoked through a symlinked path", () => {
  const scriptsDir = fileURLToPath(new URL("../scripts", import.meta.url));
  const link = join(mkdtempSync(join(tmpdir(), "sm-link-")), "scripts");
  symlinkSync(scriptsDir, link);
  const run = spawnSync(process.execPath, [join(link, "cli.mjs")], { encoding: "utf8" });
  assert.equal(run.status, 1);
  assert.match(run.stdout + run.stderr, /Usage: secrets-manager/);
});

test('byEmail, verify, validate and orphan table reads use the adapter contract', async () => {
 writeGoogleFile('base@example.com:pw');
 assert.equal(await main(['import','google'], io()), 0);
 const output=io();
 assert.equal(await main(['login','beta','--by-email'], output), 0);
 assert.ok(output.text().includes('fixture byEmail'));
 store.saveSession(db,'beta','base@example.com',[],[]);
 assert.equal(await main(['verify','beta'], io()), 0);
 assert.equal(store.getSession(db,'beta','base@example.com').status,'expired');
 const validated=io();
 assert.equal(await main(['validate',fileURLToPath(new URL('./fixtures/adapters.mjs',import.meta.url))],validated),0);
 assert.ok(validated.text().includes('alpha, beta'));
 store.saveSession(db,'orphan','base@example.com',[],[]);
 process.env.SECRETS_MANAGER_ADAPTERS='';
 assert.equal(await main(['get','orphan','--select','base@example.com'],io()),0);
 assert.equal(await main(['list'],io()),0);
 assert.equal(await main(['login','orphan'],io()),1);
});

test('setup --check reports dependencies without a global launcher', () => {
 const home=mkdtempSync(join(tmpdir(),'setup-'));
 const result=spawnSync('sh',[fileURLToPath(new URL('../scripts/setup.sh',import.meta.url)),'--check'],{env:{...process.env,HOME:home},encoding:'utf8'});
 assert.equal(result.status,0);
 assert.doesNotMatch(result.stdout,/launcher:/);
 assert.match(result.stdout,/camoufox:/);
 assert.equal(spawnSync('test',['-e',join(home,'.local/bin/secrets-manager')]).status,1);
});

test("login <app> never retries an account the app restricted, even with --all", async () => {
  writeGoogleFile("a@x.com:pw:SECRET:\n");
  store.upsertAccount(db, "a@x.com", "pw", "SECRET");
  store.setAccountStatus(db, "a@x.com", "active");
  store.recordSessionStatus(db, "alpha", "a@x.com", store.STATUS_RESTRICTED);
  const o = io();
  assert.equal(await main(["login", "alpha", "--all"], o), 0);
  assert.ok(o.text().includes("skip a@x.com: alpha status restricted"));
});


test("ready sessions are usable for export and skipped by default login without a browser", async () => {
  writeGoogleFile("a@x.com:pw:SECRET:\n");
  store.upsertAccount(db, "a@x.com", "pw", "SECRET");
  store.setAccountStatus(db, "a@x.com", "ready");
  store.saveSession(db, "beta", "a@x.com", [{ name: "auth-refresh-token", value: "READY" }], []);
  store.setSessionStatus(db, "beta", "a@x.com", "ready");
  assert.equal(needsRefresh(db, "beta", "a@x.com"), false);
  assert.equal(loginSkipReason("ready", "beta"), null);
  assert.equal(loginSkipReason("ready", "google"), null);
  const login = io();
  assert.equal(await main(["login", "beta"], login), 0);
  assert.deepEqual(login.out, ["a@x.com: beta up to date, skipping"]);
  const output = io();
  assert.equal(await main(["export", "beta", "--select", "a@x.com"], output), 0);
  assert.deepEqual(output.out.map(JSON.parse), [{ app: "beta", email: "a@x.com", refresh_token: "READY" }]);
  assert.equal(store.getSession(db, "beta", "a@x.com").status, "ready");
});


// Runtime fixture modules stay in the temporary state directory; these hooks never open a browser.
function setupAdapter(hook, setupFlags = {}) {
  const path = join(base, "setup-adapter.mjs");
  writeFileSync(path, `export default kit => {
    let running = 0, peak = 0;
    return [{
      name: 'alpha', domain: 'alpha.example', startUrl: 'https://alpha.example/',
      entryTexts: ['Login'], signIn: async () => {}, ready: async () => true,
      setup: ${hook}, setupFlags: ${JSON.stringify(setupFlags)},
    }];
  };`);
  process.env.SECRETS_MANAGER_ADAPTERS = path;
}

function setupSession(email, status, cookies = []) {
  store.upsertAccount(db, email, "pw", null);
  store.saveSession(db, "alpha", email, cookies, []);
  store.setSessionStatus(db, "alpha", email, status);
}

test("setup parses the same flags as verify and requires one app", () => {
  const flags = ["alpha", "--select", "a@x.com", "--select", "b@x.com", "--all", "--concurrency", "2", "--headed", "--rotate-proxy"];
  assert.deepEqual(parseCli(["setup", ...flags]).opts, parseCli(["verify", ...flags]).opts);
  assert.throws(() => parseCli(["setup"]), /Usage/);
  assert.throws(() => parseCli(["setup", "alpha", "beta"]), /Usage/);
});

test("setup succeeds for active sessions; --all adds ready and --select narrows", async () => {
  setupAdapter("async ({ session }) => ({ summary: `configured from ${session.status}`, state: { from: session.status } })");
  for (const status of ["new", "active", "ready", "expired", "restricted", "escalated"]) setupSession(`${status}@x.com`, status);
  store.upsertAccount(db, "missing@x.com", "pw", null);
  // As with verify, an app session needs an imported account to be selected.
  store.saveSession(db, "alpha", "orphan@x.com", [], []);
  const output = io();
  assert.equal(await main(["setup", "alpha"], output), 0);
  assert.deepEqual(output.out, ["active@x.com: configured from active"]);
  assert.equal(store.getSession(db, "alpha", "active@x.com").status, "ready");
  const skipped = io();
  assert.equal(await main(["setup", "alpha", "--select", "ready@x.com"], skipped), 0);
  assert.ok(!skipped.text().includes("configured"));
  const all = io();
  assert.equal(await main(["setup", "alpha", "--all", "--select", "active@x.com", "--select", "ready@x.com", "--select", "expired@x.com"], all), 0);
  assert.deepEqual(all.out, ["active@x.com: configured from ready", "ready@x.com: configured from ready"]);
  for (const status of ["new", "expired", "restricted", "escalated"]) assert.equal(store.getSession(db, "alpha", `${status}@x.com`).status, status);
  assert.equal(store.getSession(db, "alpha", "orphan@x.com").status, "active");
});

test("setup throws per account, preserves failed rows and continues with exit 1", async () => {
  setupAdapter("async ({ email }) => { if (email.startsWith('bad')) throw new Error('setup failed'); return { summary: 'configured', state: { configured: true } }; }");
  setupSession("bad-active@x.com", "active");
  setupSession("bad-ready@x.com", "ready");
  setupSession("good@x.com", "active");
  const before = db.prepare("SELECT * FROM alpha WHERE email LIKE 'bad%' ORDER BY email").all();
  const output = io();
  assert.equal(await main(["setup", "alpha", "--all"], output), 1);
  assert.deepEqual(output.err, ["bad-active@x.com: setup failed", "bad-ready@x.com: setup failed"]);
  assert.deepEqual(output.out, ["good@x.com: configured"]);
  assert.deepEqual(db.prepare("SELECT * FROM alpha WHERE email LIKE 'bad%' ORDER BY email").all(), before);
  assert.equal(store.getSession(db, "alpha", "good@x.com").status, "ready");
});

test("setup NeedsHuman escalates the session while plain errors preserve status and state", async () => {
  setupAdapter("async ({ email }) => { if (email.startsWith('human')) throw new kit.NeedsHuman('finish by hand'); throw new Error('setup failed'); }");
  for (const [email, status] of [["human@x.com", "active"], ["plain@x.com", "ready"]]) {
    setupSession(email, status);
    db.prepare("UPDATE alpha SET state = ? WHERE email = ?").run(JSON.stringify({ wallet: "existing" }), email);
  }
  const before = db.prepare("SELECT * FROM alpha ORDER BY email").all();
  const output = io();
  assert.equal(await main(["setup", "alpha", "--all"], output), 1);
  assert.deepEqual(output.err, ["human@x.com: escalated: finish by hand", "plain@x.com: setup failed"]);
  assert.deepEqual(output.out, []);
  const after = db.prepare("SELECT * FROM alpha ORDER BY email").all();
  assert.deepEqual({ ...after[0], updated_at: before[0].updated_at }, { ...before[0], status: "escalated" });
  assert.deepEqual(after[1], before[1]);
});

test("setup fails explicitly without a hook even when there are no sessions", async () => {
  const output = io();
  assert.equal(await main(["setup", "alpha"], output), 1);
  assert.deepEqual(output.err, ["alpha has no setup hook"]);
  assert.deepEqual(output.out, []);
});

test("setup rejects missing or empty summaries without changing status", async () => {
  setupAdapter("async ({ session }) => ({ summary: session.cookies[0].value, state: null })");
  for (const [i, value] of [undefined, null, 7, "", "   "].entries()) setupSession(`bad${i}@x.com`, "active", [{ name: "summary", value }]);
  const output = io();
  assert.equal(await main(["setup", "alpha"], output), 1);
  assert.equal(output.err.length, 5);
  assert.ok(output.err.every(line => line.endsWith(": invalid setup summary")));
  assert.deepEqual(output.out, []);
  assert.ok(db.prepare("SELECT status FROM alpha").all().every(row => row.status === "active"));
});

test("setup passes the hook context and honors concurrency", async () => {
  setupAdapter(`async ({ db, email, session, opts, io }) => {
    if (kit.store.getSession(db, 'alpha', email).status !== 'active' || session.email !== email ||
        !opts.headed || !opts['rotate-proxy'] || opts.concurrency !== 2 || typeof io.log !== 'function') throw new Error('bad context');
    peak = Math.max(peak, ++running);
    await new Promise(resolve => setImmediate(resolve));
    running--;
    return { summary: 'configured (peak ' + peak + ')', state: { peak } };
  }`);
  for (const email of ["a@x.com", "b@x.com", "c@x.com", "d@x.com"]) setupSession(email, "active");
  const output = io();
  assert.equal(await main(["setup", "alpha", "--concurrency", "2", "--headed", "--rotate-proxy"], output), 0);
  assert.equal(output.out.length, 4);
  assert.ok(output.out.every(line => line.endsWith(": configured (peak 2)")));
  assert.ok(db.prepare("SELECT status FROM alpha").all().every(row => row.status === "ready"));
});


for (const state of [null, false, 0, "wallet", [1, null], { wallet: { id: "test" }, steps: [true] }]) {
  test(`setup stores JSON state ${JSON.stringify(state)} and ready in one write; get parses it`, async () => {
    setupAdapter(`async () => ({ summary: 'configured', state: ${JSON.stringify(state)} })`);
    setupSession("a@x.com", "active");
    // A trigger rejects an intermediate ready row, proving state and status arrive together.
    db.exec(`CREATE TRIGGER require_state BEFORE UPDATE ON alpha
      WHEN NEW.status = 'ready' AND NEW.state IS NULL
      BEGIN SELECT RAISE(ABORT, 'ready needs setup state'); END`);
    const output = io();
    assert.equal(await main(["setup", "alpha", "--select", "a@x.com"], output), 0);
    assert.deepEqual(output.out, ["a@x.com: configured"]);
    assert.deepEqual(output.err, []);
    const row = db.prepare("SELECT * FROM alpha").get();
    assert.equal(row.status, "ready");
    assert.equal(row.state, JSON.stringify(state));
    const got = io();
    assert.equal(await main(["get", "alpha", "--select", "a@x.com"], got), 0);
    assert.deepEqual(JSON.parse(got.text()).state, state);
  });
}

for (const [label, result, error] of [
  ["undefined result", "undefined", "result"],
  ["null result", "null", "result"],
  ["old string contract", "'configured'", "result"],
  ["array result", "[]", "result"],
  ["missing summary", "({ state: null })", "summary"],
  ["multiline summary", "({ summary: 'line\\nline', state: null })", "summary"],
  ["carriage return summary", "({ summary: 'line\\rline', state: null })", "summary"],
  ["missing state", "({ summary: 'configured' })", "state"],
  ["undefined state", "({ summary: 'configured', state: undefined })", "state"],
  ["function state", "({ summary: 'configured', state: () => {} })", "state"],
  ["symbol state", "({ summary: 'configured', state: Symbol('state') })", "state"],
  ["BigInt state", "({ summary: 'configured', state: { id: 1n } })", "state"],
  ["circular state", "(() => { const state = {}; state.self = state; return { summary: 'configured', state }; })()", "state"],
  ["toJSON returns undefined", "({ summary: 'configured', state: { toJSON: () => undefined } })", "state"],
  ["toJSON throws", "({ summary: 'configured', state: { toJSON: () => { throw new Error('cannot encode'); } } })", "state"],
]) {
  test(`setup rejects ${label} without storing anything or changing status`, async () => {
    setupAdapter(`async () => ${result}`);
    setupSession("active@x.com", "active");
    setupSession("ready@x.com", "ready");
    db.prepare("UPDATE alpha SET state = ? WHERE email = 'ready@x.com'").run('{"previous":true}');
    const before = db.prepare("SELECT * FROM alpha ORDER BY email").all();
    const output = io();
    assert.equal(await main(["setup", "alpha", "--all"], output), 1);
    assert.deepEqual(output.out, []);
    assert.equal(output.err.length, 2);
    assert.ok(output.err.every(line => line.endsWith(`: invalid setup ${error}`)), output.errText());
    assert.deepEqual(db.prepare("SELECT * FROM alpha ORDER BY email").all(), before);
  });
}

test("a failed setup write preserves both state and status", async () => {
  setupAdapter("async () => ({ summary: 'configured', state: { wallet: 'new' } })");
  setupSession("a@x.com", "active");
  db.exec(`CREATE TRIGGER reject_setup BEFORE UPDATE ON alpha
    BEGIN SELECT RAISE(ABORT, 'write rejected'); END`);
  const before = db.prepare("SELECT * FROM alpha").get();
  const output = io();
  assert.equal(await main(["setup", "alpha"], output), 1);
  assert.match(output.errText(), /write rejected/);
  assert.deepEqual(output.out, []);
  assert.deepEqual(db.prepare("SELECT * FROM alpha").get(), before);
});

const appSetupFlags = {
  "follow-lowest-ranked": { type: "boolean", description: "Follow the lowest ranked account." },
  "wallet-name": { type: "string", description: "Name of the wallet to create." },
};

test("setup app flags reach opts with their declared types", async () => {
  setupAdapter(`async ({ opts }) => ({ summary: 'configured', state: {
    follow: opts['follow-lowest-ranked'], wallet: opts['wallet-name'], headed: opts.headed
  } })`, appSetupFlags);
  setupSession("a@x.com", "active");
  const output = io();
  assert.equal(await main(["setup", "alpha", "--follow-lowest-ranked", "--wallet-name", "test wallet", "--headed"], output), 0, output.errText());
  assert.deepEqual(store.getSession(db, "alpha", "a@x.com").state, { follow: true, wallet: "test wallet", headed: true });
});

test("setup flags belong only to their app and do not extend other commands", async () => {
  setupAdapter("async () => { throw new Error('hook must not run'); }", appSetupFlags);
  const path = join(base, "other-adapter.mjs");
  writeFileSync(path, `export default () => [{ name: 'beta', domain: 'beta.example', startUrl: 'https://beta.example/',
    entryTexts: ['Login'], signIn: async () => {}, ready: async () => true,
    setup: async () => { throw new Error('hook must not run'); } }];`);
  process.env.SECRETS_MANAGER_ADAPTERS += ':' + path;
  setupSession("a@x.com", "active");
  for (const args of [["setup", "beta"], ["login", "alpha"], ["verify", "alpha"]]) {
    const output = io();
    assert.equal(await main([...args, "--follow-lowest-ranked"], output), 1);
    assert.match(output.errText(), /Unknown option.*follow-lowest-ranked/);
    assert.doesNotMatch(output.errText(), /hook must not run/);
    assert.equal(store.getSession(db, "alpha", "a@x.com").status, "active");
  }
});

test("setup help and unknown flags list app descriptions before the hook runs", async () => {
  setupAdapter("async () => { throw new Error('hook must not run'); }", appSetupFlags);
  setupSession("a@x.com", "active");
  for (const flag of ["--help", "--follow-lowest-rankd", "--unknown"]) {
    const output = io();
    assert.equal(await main(["setup", "alpha", flag], output), flag === "--help" ? 0 : 1);
    const text = flag === "--help" ? output.text() : output.errText();
    assert.match(text, /setup <app>.*\[app flags\]/);
    for (const [name, { description }] of Object.entries(appSetupFlags)) {
      assert.ok(text.includes('--' + name), text);
      assert.ok(text.includes(description), text);
    }
    if (flag !== "--help") assert.match(text, /Unknown option/);
    assert.doesNotMatch(text, /hook must not run/);
    assert.equal(store.getSession(db, "alpha", "a@x.com").status, "active");
  }
});

for (const state of [null, false, 0, "wallet", [1, null], { wallet: { id: "previous" } }]) {
  test(`setup hook receives parsed previous state ${JSON.stringify(state)}`, async () => {
    setupAdapter("async ({ session }) => ({ summary: 'configured', state: { previous: session.state } })");
    setupSession("a@x.com", "active");
    if (state !== null) store.saveSetupState(db, "alpha", "a@x.com", state);
    const output = io();
    assert.equal(await main(["setup", "alpha", "--all"], output), 0, output.errText());
    assert.deepEqual(store.getSession(db, "alpha", "a@x.com").state, { previous: state });
  });
}
