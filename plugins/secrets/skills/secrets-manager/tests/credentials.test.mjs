// ABOUTME: Tests the colon-separated credential parsing, directory loading and app-password write-back.
// ABOUTME: Uses a temp directory; no browser and no network.

import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { parseLine, loadCredentials, setAppPassword, setTotpSecret, totpUrlPatterns } from "../scripts/credentials.mjs";

const tmp = () => mkdtempSync(join(tmpdir(), "creds-"));

test("parseLine reads a full line", () => {
  const cred = parseLine("a@x.com:pw:SECRET:kaca xozm ltvz vmtf");
  assert.deepEqual(cred, {
    email: "a@x.com",
    password: "pw",
    totp_secret: "SECRET",
    app_password: "kaca xozm ltvz vmtf",
  });
});

test("parseLine treats a trailing empty app password as none", () => {
  const cred = parseLine("a@x.com:pw:SECRET:");
  assert.equal(cred.totp_secret, "SECRET");
  assert.equal(cred.app_password, null);
});

test("parseLine accepts only email and password", () => {
  const cred = parseLine("a@x.com:pw");
  assert.equal(cred.totp_secret, null);
  assert.equal(cred.app_password, null);
});

const VENDOR_LINE = "u@x.com:pw123:rec@vend.com:http://vend.com/view/abc123:https://totp.example/AAAA2222BBBB3333CCCC4444";

test("parseLine reads a configured vendor format (TOTP in a URL, extra fields)", () => {
  assert.deepEqual(parseLine(VENDOR_LINE, [/totp\.example\/([A-Za-z2-7]{16,})/]), {
    email: "u@x.com",
    password: "pw123",
    totp_secret: "AAAA2222BBBB3333CCCC4444",
    app_password: null,
  });
});

test("parseLine knows no vendor format unless one is configured", () => {
  assert.equal(parseLine(VENDOR_LINE, []).totp_secret, "rec@vend.com");
});

test("totpUrlPatterns reads config.json, and is empty without it", () => {
  const dir = tmp(), path = join(dir, "config.json");
  assert.deepEqual(totpUrlPatterns(path), []);
  writeFileSync(path, JSON.stringify({ adapters: [], totpUrlPatterns: ["totp\\.example/([A-Za-z2-7]{16,})"] }));
  assert.equal(parseLine(VENDOR_LINE, totpUrlPatterns(path)).totp_secret, "AAAA2222BBBB3333CCCC4444");
});

test("parseLine returns null for a blank line", () => {
  assert.equal(parseLine("   "), null);
});

test("parseLine throws without a password", () => {
  assert.throws(() => parseLine("a@x.com"), /no password/);
});

test("loadCredentials of a missing directory is empty", () => {
  assert.deepEqual(loadCredentials(join(tmp(), "nope")), []);
});

test("loadCredentials reads all lines across files, sorted by filename", () => {
  const dir = tmp();
  writeFileSync(join(dir, "b.txt"), "b@x.com:pw:S:\n\nc@x.com:pw2:T:aaaa bbbb cccc dddd\n");
  writeFileSync(join(dir, "a.txt"), "a@x.com:pw:S:code word here now\n");
  assert.deepEqual(loadCredentials(dir).map((c) => c.email), ["a@x.com", "b@x.com", "c@x.com"]);
});

test("a later file overrides the same email", () => {
  const dir = tmp();
  writeFileSync(join(dir, "a.txt"), "dup@x.com:old:S:\n");
  writeFileSync(join(dir, "b.txt"), "dup@x.com:new:T:\n");
  const creds = loadCredentials(dir);
  assert.equal(creds.length, 1);
  assert.equal(creds[0].password, "new");
});

test("loadCredentials ignores non-txt files", () => {
  const dir = tmp();
  writeFileSync(join(dir, "a.txt"), "a@x.com:pw:S:\n");
  writeFileSync(join(dir, "notes.md"), "ignored");
  assert.deepEqual(loadCredentials(dir).map((c) => c.email), ["a@x.com"]);
});

test("a bad line names its file and line number", () => {
  const dir = tmp();
  writeFileSync(join(dir, "bad.txt"), "ok@x.com:pw:S:\nbroken\n");
  assert.throws(() => loadCredentials(dir), /bad\.txt:2/);
});

test("setAppPassword replaces an existing app password", () => {
  const dir = tmp();
  writeFileSync(join(dir, "a.txt"), "a@x.com:pw:SEC:old old old old\nb@x.com:pw2:T:keep keep keep keep\n");
  assert.equal(setAppPassword(dir, "a@x.com", "new1 new2 new3 new4"), true);
  const creds = Object.fromEntries(loadCredentials(dir).map((c) => [c.email, c]));
  assert.equal(creds["a@x.com"].app_password, "new1 new2 new3 new4");
  assert.equal(creds["a@x.com"].password, "pw");
  assert.equal(creds["a@x.com"].totp_secret, "SEC");
  assert.equal(creds["b@x.com"].app_password, "keep keep keep keep");
});

test("setAppPassword fills a missing fourth field", () => {
  const dir = tmp();
  writeFileSync(join(dir, "a.txt"), "a@x.com:pw:SEC\n");
  assert.equal(setAppPassword(dir, "a@x.com", "abcd efgh ijkl mnop"), true);
  const cred = loadCredentials(dir)[0];
  assert.equal(cred.app_password, "abcd efgh ijkl mnop");
  assert.equal(cred.totp_secret, "SEC");
});

test("setAppPassword returns false for an unknown email and leaves the file alone", () => {
  const dir = tmp();
  writeFileSync(join(dir, "a.txt"), "a@x.com:pw:SEC:x x x x\n");
  assert.equal(setAppPassword(dir, "missing@x.com", "y y y y"), false);
  assert.equal(readFileSync(join(dir, "a.txt"), "utf8"), "a@x.com:pw:SEC:x x x x\n");
});

test("setTotpSecret fills a missing third field", () => {
  const dir = tmp();
  writeFileSync(join(dir, "a.txt"), "a@x.com:pw\n");
  assert.equal(setTotpSecret(dir, "a@x.com", "NEWSECRET"), true);
  const cred = loadCredentials(dir)[0];
  assert.equal(cred.totp_secret, "NEWSECRET");
  assert.equal(cred.password, "pw");
});

test("setTotpSecret preserves an existing app password", () => {
  const dir = tmp();
  writeFileSync(join(dir, "a.txt"), "a@x.com:pw::keep keep keep keep\n");
  assert.equal(setTotpSecret(dir, "a@x.com", "SEC"), true);
  const cred = loadCredentials(dir)[0];
  assert.equal(cred.totp_secret, "SEC");
  assert.equal(cred.app_password, "keep keep keep keep");
});

test("setTotpSecret replaces an existing secret and leaves other lines alone", () => {
  const dir = tmp();
  writeFileSync(join(dir, "a.txt"), "a@x.com:pw:OLD:app app app app\nb@x.com:pw2:T:\n");
  assert.equal(setTotpSecret(dir, "a@x.com", "NEW"), true);
  const creds = Object.fromEntries(loadCredentials(dir).map((c) => [c.email, c]));
  assert.equal(creds["a@x.com"].totp_secret, "NEW");
  assert.equal(creds["a@x.com"].app_password, "app app app app");
  assert.equal(creds["b@x.com"].totp_secret, "T");
});

test("setTotpSecret returns false for an unknown email and leaves the file alone", () => {
  const dir = tmp();
  writeFileSync(join(dir, "a.txt"), "a@x.com:pw:SEC:x x x x\n");
  assert.equal(setTotpSecret(dir, "missing@x.com", "Y"), false);
  assert.equal(readFileSync(join(dir, "a.txt"), "utf8"), "a@x.com:pw:SEC:x x x x\n");
});
