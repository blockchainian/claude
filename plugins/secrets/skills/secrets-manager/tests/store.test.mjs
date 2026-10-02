// ABOUTME: Tests the SQLite store: the google table, per-app session tables, status rules, and
// ABOUTME: the x-table functions. No browser, no network.

import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

import * as store from "../scripts/store.mjs";

const open = () => store.openDb(":memory:");

test("a newly imported google account starts as 'new'", () => {
  const db = open();
  store.upsertAccount(db, "a@x.com", "pw", null, null);
  assert.equal(store.getAccount(db, "a@x.com").status, store.STATUS_NEW);
});

test("markLoggedIn promotes a new account to active", () => {
  const db = open();
  store.upsertAccount(db, "a@x.com", "pw", null, null);
  store.markLoggedIn(db, "a@x.com");
  assert.equal(store.getAccount(db, "a@x.com").status, store.STATUS_ACTIVE);
});

test("setAccountStatus accepts the new status", () => {
  const db = open();
  store.upsertAccount(db, "a@x.com", "pw", null, null);
  store.setAccountStatus(db, "a@x.com", store.STATUS_NEW);
  assert.equal(store.getAccount(db, "a@x.com").status, store.STATUS_NEW);
});

test("openDb migrates a pre-'new' google table, preserving rows and allowing 'new'", () => {
  const path = join(mkdtempSync(join(tmpdir(), "store-")), "s.sqlite");
  const raw = new DatabaseSync(path);
  raw.exec(`CREATE TABLE google (
    email TEXT PRIMARY KEY, password TEXT NOT NULL, totp_secret TEXT, app_password TEXT,
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','expired','restricted','escalated')),
    profile_dir TEXT, proxy TEXT, last_login_at TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
  )`);
  raw
    .prepare("INSERT INTO google (email, password, status, created_at, updated_at) VALUES (?, ?, ?, ?, ?)")
    .run("old@x.com", "pw", "escalated", "t", "t");
  raw.close();
  const db = store.openDb(path); // opening runs the migration
  assert.equal(store.getAccount(db, "old@x.com").status, "escalated"); // existing row preserved
  store.upsertAccount(db, "fresh@x.com", "pw", null, null); // 'new' would violate the old CHECK
  assert.equal(store.getAccount(db, "fresh@x.com").status, store.STATUS_NEW);
  db.close();
});

test("upsertAccount returns true only when new", () => {
  const db = open();
  const first = store.upsertAccount(db, "a@x.com", "pw", "SECRET", "app pass word here");
  const again = store.upsertAccount(db, "a@x.com", "pw2", "SECRET", null);
  assert.equal(first, true);
  assert.equal(again, false);
  assert.equal(store.getAccount(db, "a@x.com").password, "pw2");
});

test("stores app_password and totp", () => {
  const db = open();
  store.upsertAccount(db, "a@x.com", "pw", "SECRET", "aaaa bbbb cccc dddd");
  const acct = store.getAccount(db, "a@x.com");
  assert.equal(acct.totp_secret, "SECRET");
  assert.equal(acct.app_password, "aaaa bbbb cccc dddd");
});

test("re-upsert preserves status", () => {
  const db = open();
  store.upsertAccount(db, "a@x.com", "pw", "S", null);
  store.setAccountStatus(db, "a@x.com", store.STATUS_ESCALATED);
  store.upsertAccount(db, "a@x.com", "pw", "S", null);
  assert.equal(store.getAccount(db, "a@x.com").status, store.STATUS_ESCALATED);
});

test("account status must be known", () => {
  const db = open();
  store.upsertAccount(db, "a@x.com", "pw", "S", null);
  assert.throws(() => store.setAccountStatus(db, "a@x.com", "sleeping"), /unknown status/);
});

test("the CHECK constraint rejects an unknown status", () => {
  const db = open();
  assert.throws(() =>
    db.exec(
      "INSERT INTO google (email, password, status, created_at, updated_at) " +
        "VALUES ('b@x.com', 'pw', 'sleeping', 't', 't')",
    ),
  );
});

test("each app gets its own bare table", () => {
  const db = open();
  store.ensureAppTable(db, "Cool App");
  store.ensureAppTable(db, "other");
  const tables = new Set(
    db.prepare("SELECT name FROM sqlite_master WHERE type='table'").all().map((r) => r.name),
  );
  assert.ok(tables.has("cool_app"));
  assert.ok(tables.has("other"));
  assert.deepEqual(store.listApps(db), ["cool_app", "other"]);
});

test("listApps excludes google and x", () => {
  const db = open();
  store.ensureAppTable(db, "alpha");
  assert.deepEqual(store.listApps(db), ["alpha"]);
});

test("listApps ignores tables that lack the session shape", () => {
  const db = open();
  store.ensureAppTable(db, "alpha");
  // A stray table (e.g. left by a mistyped sqlite command) has no email column.
  db.exec('CREATE TABLE "em" ("stray@example.com" TEXT)');
  assert.deepEqual(store.listApps(db), ["alpha"]);
  assert.doesNotThrow(() => store.sessionsForAccount(db, "a@x.com"));
});

test("toAppSlug rejects an empty slug", () => {
  assert.throws(() => store.toAppSlug("!!!"), /no usable characters/);
});

test("save and get session round-trips JSON", () => {
  const db = open();
  const cookies = [{ name: "SID", value: "abc" }];
  const ls = { "https://foo.app": { token: "t" } };
  store.saveSession(db, "foo", "a@x.com", cookies, ls);
  const got = store.getSession(db, "foo", "a@x.com");
  assert.deepEqual(got.cookies, cookies);
  assert.deepEqual(got.local_storage, ls);
  assert.equal(got.status, store.STATUS_ACTIVE);
  assert.equal(got.app, "foo");
});

test("re-save preserves created_at", () => {
  const db = open();
  store.saveSession(db, "foo", "a@x.com", [], {});
  const created = db.prepare('SELECT created_at FROM "foo" WHERE email = ?').get("a@x.com").created_at;
  store.saveSession(db, "foo", "a@x.com", [{ name: "new" }], {});
  const again = db.prepare('SELECT created_at FROM "foo" WHERE email = ?').get("a@x.com").created_at;
  assert.equal(created, again);
});

test("two apps keep separate state for the same email", () => {
  const db = open();
  store.saveSession(db, "foo", "a@x.com", [{ name: "foo" }], {});
  store.saveSession(db, "bar", "a@x.com", [{ name: "bar" }], {});
  assert.deepEqual(store.getSession(db, "foo", "a@x.com").cookies, [{ name: "foo" }]);
  assert.deepEqual(store.getSession(db, "bar", "a@x.com").cookies, [{ name: "bar" }]);
});

test("blocking one app leaves the other usable", () => {
  const db = open();
  store.saveSession(db, "foo", "a@x.com", [], {});
  store.saveSession(db, "bar", "a@x.com", [], {});
  store.setSessionStatus(db, "foo", "a@x.com", store.STATUS_RESTRICTED);
  assert.deepEqual(store.sessionsForAccount(db, "a@x.com"), {
    bar: store.STATUS_ACTIVE,
    foo: store.STATUS_RESTRICTED,
  });
});

test("session status must be known", () => {
  const db = open();
  store.saveSession(db, "foo", "a@x.com", [], {});
  assert.throws(() => store.setSessionStatus(db, "foo", "a@x.com", "meh"), /unknown status/);
});

test("marking a missing session throws", () => {
  const db = open();
  store.ensureAppTable(db, "foo");
  assert.throws(
    () => store.setSessionStatus(db, "foo", "nobody@x.com", store.STATUS_EXPIRED),
    /has no foo session/,
  );
});

test("markLoggedIn sets last_login_at", () => {
  const db = open();
  store.upsertAccount(db, "a@x.com", "pw", null, null);
  assert.equal(store.getAccount(db, "a@x.com").last_login_at, null);
  store.markLoggedIn(db, "a@x.com");
  assert.match(store.getAccount(db, "a@x.com").last_login_at, /^\d{4}-\d{2}-\d{2}T/);
});

test("markLoggedIn clears an escalation back to active", () => {
  const db = open();
  store.upsertAccount(db, "a@x.com", "pw", null, null);
  store.setAccountStatus(db, "a@x.com", store.STATUS_ESCALATED);
  store.markLoggedIn(db, "a@x.com");
  assert.equal(store.getAccount(db, "a@x.com").status, store.STATUS_ACTIVE);
});

test("listAccounts is ordered by email", () => {
  const db = open();
  store.upsertAccount(db, "b@x.com", "pw", null, null);
  store.upsertAccount(db, "a@x.com", "pw", null, null);
  assert.deepEqual(store.listAccounts(db).map((r) => r.email), ["a@x.com", "b@x.com"]);
});

test("re-importing an X account with another auth_token drops the ct0 and cookies of the old one", () => {
  const db = open();
  store.upsertX(db, { username: "bob", password: "p", email: "b@x.com", auth_token: "old-token" });
  store.saveXLogin(db, "bob", { authToken: "old-token", ct0: "old-ct0", cookies: { auth_token: "old-token", ct0: "old-ct0" } });
  store.upsertX(db, { username: "bob", password: "p", email: "b@x.com", auth_token: "fresh-token" });
  const row = db.prepare("SELECT auth_token, ct0, cookies FROM x WHERE username = 'bob'").get();
  assert.deepEqual({ ...row }, { auth_token: "fresh-token", ct0: null, cookies: null });
  assert.deepEqual(store.getVerifiable(db).map((r) => r.username), ["bob"]);
});

test("re-importing an X account with the same auth_token, or none, keeps its ct0 and cookies", () => {
  const db = open();
  store.upsertX(db, { username: "bob", password: "p", email: "b@x.com", auth_token: "token" });
  store.saveXLogin(db, "bob", { authToken: "token", ct0: "ct0", cookies: { auth_token: "token", ct0: "ct0" } });
  store.upsertX(db, { username: "bob", password: "p", email: "b@x.com", auth_token: "token" });
  store.upsertX(db, { username: "bob", password: "p2", email: "b@x.com" });
  const row = db.prepare("SELECT auth_token, ct0, cookies FROM x WHERE username = 'bob'").get();
  assert.deepEqual({ ...row }, { auth_token: "token", ct0: "ct0", cookies: '{"auth_token":"token","ct0":"ct0"}' });
});

// --- tiktok logins ---------------------------------------------------------------

test("an imported tiktok account starts 'new' and pending", () => {
  const db = open();
  store.upsertTiktok(db, { username: "bob1", password: "pw", email: "bob@mail.com", email_password: "epw" });
  const row = store.getTiktok(db, "bob1");
  assert.equal(row.status, store.STATUS_NEW);
  assert.equal(row.cookies, null);
  assert.deepEqual(store.getPendingTiktok(db).map((r) => r.username), ["bob1"]);
});

test("saveTiktokLogin stores the cookies and the ISP slot, and the account stops being pending", () => {
  const db = open();
  store.upsertTiktok(db, { username: "bob1", password: "pw" });
  store.saveTiktokLogin(db, "bob1", { cookies: [{ name: "sessionid", value: "S" }], ispSlot: 10 });
  const row = store.getTiktok(db, "bob1");
  assert.equal(row.status, store.STATUS_ACTIVE);
  assert.equal(row.isp_slot, 10);
  assert.deepEqual(JSON.parse(row.cookies), [{ name: "sessionid", value: "S" }]);
  assert.deepEqual(store.getPendingTiktok(db), []);
  assert.equal(store.getPendingTiktok(db, { force: true }).length, 1);
});

test("an expired tiktok session is pending again; other statuses with a session are not", () => {
  const db = open();
  store.upsertTiktok(db, { username: "bob1", password: "pw" });
  store.saveTiktokLogin(db, "bob1", { cookies: [], ispSlot: 10 });
  store.setTiktokStatus(db, "bob1", store.STATUS_RESTRICTED);
  assert.deepEqual(store.getPendingTiktok(db), []);
  store.setTiktokStatus(db, "bob1", store.STATUS_EXPIRED);
  assert.equal(store.getPendingTiktok(db).length, 1);
});

test("tiktok status must be known and the account must exist", () => {
  const db = open();
  assert.throws(() => store.setTiktokStatus(db, "nobody", store.STATUS_EXPIRED), /no TikTok account/);
  assert.throws(() => store.saveTiktokLogin(db, "nobody", { cookies: [], ispSlot: 1 }), /no TikTok account/);
  store.upsertTiktok(db, { username: "bob1", password: "pw" });
  assert.throws(() => store.setTiktokStatus(db, "bob1", "sleeping"), /unknown status/);
});

test("listApps excludes tiktok", () => {
  const db = open();
  store.upsertTiktok(db, { username: "bob1", password: "pw", email: "bob@mail.com" });
  assert.deepEqual(store.listApps(db), []);
});
