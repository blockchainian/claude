import { test, beforeEach, afterEach } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { main } from "../scripts/cli.mjs";
import * as store from "../scripts/store.mjs";

let base, db, previous;
beforeEach(() => {
  previous = { HOME: process.env.HOME, SECRETS_STATE_DIR: process.env.SECRETS_STATE_DIR };
  base = mkdtempSync(join(tmpdir(), "sm-export-"));
  process.env.HOME = base;
  process.env.SECRETS_STATE_DIR = base;
  const configDir = join(base, ".config", "secrets-manager");
  mkdirSync(configDir, { recursive: true });
  writeFileSync(join(configDir, "config.json"), JSON.stringify({ adapters: [] }));
  db = store.openDb(join(base, "secrets.sqlite"));
});
afterEach(() => {
  db.close();
  for (const [key, value] of Object.entries(previous)) {
    if (value === undefined) delete process.env[key];
    else process.env[key] = value;
  }
  rmSync(base, { recursive: true, force: true });
});

const output = () => ({ out: [], err: [], log(s) { this.out.push(s); }, error(s) { this.err.push(s); } });
const cookies = { auth_token: "fixture-auth", ct0: "fixture-ct0", lang: "en" };
function addX(username, status = "active") {
  store.upsertX(db, { username, email: `${username}@example.com`, password: "fixture-password" });
  store.saveXLogin(db, username, { authToken: cookies.auth_token, ct0: cookies.ct0, cookies });
  store.setXStatus(db, username, status);
}

test("export x prints active accounts as JSONL and skips every non-active status", async () => {
  addX("bob");
  addX("alice");
  for (const status of ["new", "expired", "restricted", "escalated"]) addX(status, status);
  const io = output();
  assert.equal(await main(["export", "x"], io), 0, io.err.join("\n"));
  assert.deepEqual(io.out.map(line => JSON.parse(line)), ["alice", "bob"].map(username => ({
    app: "x", username, auth_token: cookies.auth_token, ct0: cookies.ct0, cookies,
  })));
  assert.deepEqual(io.err, []);
});

test("export x filters by repeated username selections without requiring Google accounts", async () => {
  for (const username of ["alice", "bob", "carol"]) addX(username);
  const io = output();
  assert.equal(await main(["export", "x", "--select", "carol", "--select", "alice"], io), 0);
  assert.deepEqual(io.out.map(line => JSON.parse(line).username), ["alice", "carol"]);
  assert.deepEqual(io.err, []);
});

test("export x reports missing required tokens and permits an absent optional cookie jar", async () => {
  for (const [username, field, value] of [["no_auth", "auth_token", null], ["no_ct0", "ct0", ""]]) {
    addX(username);
    db.prepare(`UPDATE x SET ${field} = ? WHERE username = ?`).run(value, username);
  }
  addX("pair_only");
  db.prepare("UPDATE x SET cookies = NULL WHERE username = ?").run("pair_only");
  const io = output();
  assert.equal(await main(["export", "x"], io), 0);
  assert.deepEqual(io.out.map(line => JSON.parse(line)), [{
    app: "x", username: "pair_only", auth_token: cookies.auth_token, ct0: cookies.ct0, cookies: null,
  }]);
  assert.deepEqual(io.err, ["no_auth\tmissing", "no_ct0\tmissing"]);
});

test("export tiktok prints active cookies and ISP slot, filters usernames and reports missing sessions", async () => {
  const session = [{ name: "sessionid", value: "fixture-session", domain: ".tiktok.com" }];
  for (const username of ["active", "expired", "no_cookies", "no_slot", "empty_cookies"]) {
    store.upsertTiktok(db, { username, password: "fixture-password" });
    store.saveTiktokLogin(db, username, { cookies: session, ispSlot: 0 });
  }
  store.setTiktokStatus(db, "expired", "expired");
  db.prepare("UPDATE tiktok SET cookies = NULL WHERE username = ?").run("no_cookies");
  db.prepare("UPDATE tiktok SET cookies = '[]' WHERE username = ?").run("empty_cookies");
  db.prepare("UPDATE tiktok SET isp_slot = NULL WHERE username = ?").run("no_slot");
  const io = output();
  assert.equal(await main(["export", "tiktok"], io), 0);
  assert.deepEqual(io.out.map(line => JSON.parse(line)), [{ app: "tiktok", username: "active", cookies: session, isp_slot: 0 }]);
  assert.deepEqual(io.err, ["empty_cookies\tmissing", "no_cookies\tmissing", "no_slot\tmissing"]);
  const selected = output();
  assert.equal(await main(["export", "tiktok", "--select", "active"], selected), 0);
  assert.deepEqual(selected.out, io.out);
  assert.deepEqual(selected.err, []);
});
