// ABOUTME: Tests for `verify <target>` dispatch: a builtin account check (google, x, tiktok) runs
// ABOUTME: before any adapter hook, picks rows like the adapter path, and writes back each result.

import { test, beforeEach, afterEach } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { main } from "../scripts/cli.mjs";
import { BUILTIN_CHECKS, nextStatus } from "../scripts/verify.mjs";
import * as config from "../scripts/config.mjs";
import * as store from "../scripts/store.mjs";

let db;
let realChecks;
const io = () => {
  const out = [];
  const err = [];
  return { log: (s) => out.push(s), error: (s) => err.push(s), text: () => out.join("\n"), errText: () => err.join("\n") };
};

beforeEach(() => {
  process.env.SECRETS_MANAGER_ADAPTERS = fileURLToPath(new URL("./fixtures/adapters.mjs", import.meta.url));
  process.env.SECRETS_MANAGER_STATE_PATH = mkdtempSync(join(tmpdir(), "sm-verify-"));
  db = store.openDb(config.dbPath());
  realChecks = Object.fromEntries(Object.entries(BUILTIN_CHECKS).map(([k, v]) => [k, v.check]));
});
afterEach(() => {
  for (const [k, check] of Object.entries(realChecks)) BUILTIN_CHECKS[k].check = check;
  db.close();
  delete process.env.SECRETS_MANAGER_STATE_PATH;
  delete process.env.SECRETS_MANAGER_ADAPTERS;
});

const status = (table, key, id) => db.prepare(`SELECT status FROM ${table} WHERE ${key} = ?`).get(id).status;

function seedX(...rows) {
  for (const [username, st] of rows) {
    store.upsertX(db, { username, password: "pw", email: `${username}@mail.com` });
    store.setXStatus(db, username, st);
  }
}

test("google, x and tiktok have builtin checks", () => {
  assert.deepEqual(Object.keys(BUILTIN_CHECKS).sort(), ["google", "tiktok", "x"]);
});

test("verify x runs the builtin check on active rows and writes each result back", async () => {
  seedX(["amy", "active"], ["bob", "active"], ["cat", "expired"]);
  const seen = [];
  BUILTIN_CHECKS.x.check = async ({ row }) => {
    seen.push(row.username);
    return row.username === "amy" ? "restricted" : "escalated";
  };
  const o = io();
  assert.equal(await main(["verify", "x"], o), 0);
  assert.deepEqual(seen.sort(), ["amy", "bob"]);
  assert.equal(status("x", "username", "amy"), "restricted");
  assert.equal(status("x", "username", "bob"), "escalated");
  assert.equal(status("x", "username", "cat"), "expired");
  assert.ok(o.text().includes("amy: restricted"));
});

test("verify --all includes rows that are not active; --select narrows", async () => {
  seedX(["amy", "active"], ["bob", "expired"], ["cat", "restricted"]);
  const seen = [];
  BUILTIN_CHECKS.x.check = async ({ row }) => (seen.push(row.username), "active");
  assert.equal(await main(["verify", "x", "--all", "--select", "bob", "--select", "cat"], io()), 0);
  assert.deepEqual(seen.sort(), ["bob", "cat"]);
  assert.equal(status("x", "username", "bob"), "active");
});

test("a failed or invalid check leaves the status unchanged and exits 1", async () => {
  seedX(["amy", "active"], ["bob", "active"]);
  BUILTIN_CHECKS.x.check = async ({ row }) => {
    if (row.username === "amy") throw new Error("probe inconclusive: HTTP 503");
    return "new";
  };
  const o = io();
  assert.equal(await main(["verify", "x"], o), 1);
  assert.equal(status("x", "username", "amy"), "active");
  assert.equal(status("x", "username", "bob"), "active");
  assert.ok(o.errText().includes("amy: probe inconclusive: HTTP 503"));
  assert.ok(o.errText().includes("bob: invalid verify result new"));
});

test("verify google and tiktok write to their own tables", async () => {
  store.upsertAccount(db, "g@mail.com", "pw", null);
  store.setAccountStatus(db, "g@mail.com", "active");
  store.upsertTiktok(db, { username: "tok", password: "pw" });
  store.setTiktokStatus(db, "tok", "active");
  BUILTIN_CHECKS.google.check = async ({ row }) => (row.email === "g@mail.com" ? "expired" : "active");
  BUILTIN_CHECKS.tiktok.check = async ({ row }) => (row.username === "tok" ? "restricted" : "active");
  assert.equal(await main(["verify", "google"], io()), 0);
  assert.equal(await main(["verify", "tiktok"], io()), 0);
  assert.equal(status("google", "email", "g@mail.com"), "expired");
  assert.equal(status("tiktok", "username", "tok"), "restricted");
});

test("verify google skips plus-alias rows: they hold app sessions, not a Google sign-in", async () => {
  for (const email of ["g@mail.com", "g+1@mail.com"]) {
    store.upsertAccount(db, email, "pw", null);
    store.setAccountStatus(db, email, "active");
  }
  const seen = [];
  BUILTIN_CHECKS.google.check = async ({ row }) => (seen.push(row.email), "active");
  assert.equal(await main(["verify", "google", "--all"], io()), 0);
  assert.deepEqual(seen, ["g@mail.com"]);
});

test("verify with no active rows says so and exits 0", async () => {
  const o = io();
  assert.equal(await main(["verify", "tiktok"], o), 0);
  assert.ok(o.text().includes("no TikTok active accounts to check"));
});

test("expired never overwrites restricted or escalated; every other result is written", () => {
  assert.equal(nextStatus("restricted", "expired"), "restricted");
  assert.equal(nextStatus("escalated", "expired"), "escalated");
  assert.equal(nextStatus("restricted", "active"), "active");
  assert.equal(nextStatus("escalated", "restricted"), "restricted");
  assert.equal(nextStatus("active", "expired"), "expired");
  assert.equal(nextStatus("expired", "escalated"), "escalated");
});

test("verify --all keeps a banned account restricted when only its token is dead", async () => {
  seedX(["amy", "restricted"], ["bob", "escalated"], ["cat", "active"]);
  BUILTIN_CHECKS.x.check = async () => "expired";
  const o = io();
  assert.equal(await main(["verify", "x", "--all"], o), 0);
  assert.equal(status("x", "username", "amy"), "restricted");
  assert.equal(status("x", "username", "bob"), "escalated");
  assert.equal(status("x", "username", "cat"), "expired");
  assert.ok(o.text().includes("amy: expired (kept restricted)"));
});

test("an adapter app's restricted session is kept too", async () => {
  store.upsertAccount(db, "g@mail.com", "pw", null);
  store.saveSession(db, "beta", "g@mail.com", [], []);
  store.setSessionStatus(db, "beta", "g@mail.com", "restricted");
  assert.equal(await main(["verify", "beta", "--all"], io()), 0);
  assert.equal(store.getSession(db, "beta", "g@mail.com").status, "restricted");
});
