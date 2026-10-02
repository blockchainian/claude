// ABOUTME: Tests state-path resolution (one root env var, fixed sub-paths) and the sticky
// ABOUTME: per-account proxy derivation. No browser, no network.

import { test, afterEach } from "node:test";
import assert from "node:assert/strict";
import { basename, dirname, join } from "node:path";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";

import * as config from "../scripts/config.mjs";

afterEach(() => {
  delete process.env.SECRETS_MANAGER_STATE_PATH;
  delete process.env.RESIDENTIAL_PROXY_URL;
  delete process.env.ENVFILE_TEST_A;
  delete process.env.ENVFILE_TEST_B;
});

test("default state path is ~/.config/secrets-manager", () => {
  assert.equal(basename(config.statePath()), "secrets-manager");
  assert.equal(basename(dirname(config.statePath())), ".config");
});

test("every path hangs off the state root", () => {
  process.env.SECRETS_MANAGER_STATE_PATH = "/tmp/sm";
  assert.equal(config.dbPath(), "/tmp/sm/secrets.sqlite");
  assert.equal(config.credentialsDir("google"), "/tmp/sm/google");
  assert.equal(config.credentialsDir("x"), "/tmp/sm/x");
  assert.equal(config.profileDirFor("a@x.com"), join("/tmp/sm/profiles", "a@x.com"));
  assert.equal(config.debugDir(), "/tmp/sm/debug");
});

test("a ~ state path expands to the home directory", () => {
  process.env.SECRETS_MANAGER_STATE_PATH = "~/sm";
  assert.ok(!config.statePath().startsWith("~"));
  assert.ok(config.statePath().endsWith("/sm"));
});

test("loadEnvFile sets unset keys only", () => {
  const dir = mkdtempSync(join(tmpdir(), "env-"));
  const file = join(dir, ".env");
  writeFileSync(file, "# comment\nENVFILE_TEST_A=one\nENVFILE_TEST_B=two=2\n\nnot a pair\n");
  process.env.ENVFILE_TEST_A = "kept";
  config.loadEnvFile(file);
  assert.equal(process.env.ENVFILE_TEST_A, "kept");
  assert.equal(process.env.ENVFILE_TEST_B, "two=2");
  config.loadEnvFile(join(dir, "missing")); // a missing file is a no-op
});

test("proxy defaults to null without env, else reads it", () => {
  delete process.env.RESIDENTIAL_PROXY_URL;
  assert.equal(config.defaultProxy(), null);
  process.env.RESIDENTIAL_PROXY_URL = "http://host:1";
  assert.equal(config.defaultProxy(), "http://host:1");
});

test("proxyFor injects a sticky sessid after the username", () => {
  process.env.RESIDENTIAL_PROXY_URL = "http://customer-acct-cc-US:pw@proxy.example:7777";
  const url = config.proxyFor("a@x.com");
  assert.ok(url.startsWith("http://customer-acct-cc-US-sessid-"));
  assert.ok(url.includes("-sesstime-10"));
  assert.ok(url.includes("@proxy.example:7777"));
});

test("proxyFor is deterministic per account", () => {
  process.env.RESIDENTIAL_PROXY_URL = "http://customer-acct:pw@h:1";
  assert.equal(config.proxyFor("a@x.com"), config.proxyFor("a@x.com"));
  assert.notEqual(config.proxyFor("a@x.com"), config.proxyFor("b@x.com"));
});

test("proxyFor leaves a proxy using another username convention or an existing sessid untouched", () => {
  process.env.RESIDENTIAL_PROXY_URL = "http://user:pw@h:1";
  assert.equal(config.proxyFor("a@x.com"), "http://user:pw@h:1");
  process.env.RESIDENTIAL_PROXY_URL = "http://customer-acct-sessid-x:pw@h:1";
  assert.equal(config.proxyFor("a@x.com"), "http://customer-acct-sessid-x:pw@h:1");
});

test("proxyFor returns the base when rotating", () => {
  process.env.RESIDENTIAL_PROXY_URL = "http://customer-acct:pw@h:1";
  assert.equal(config.proxyFor("a@x.com", { rotate: true }), "http://customer-acct:pw@h:1");
});

test("proxyFor is null without env", () => {
  delete process.env.RESIDENTIAL_PROXY_URL;
  assert.equal(config.proxyFor("a@x.com"), null);
});
