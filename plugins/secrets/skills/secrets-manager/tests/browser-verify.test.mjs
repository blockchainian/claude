// ABOUTME: Tests for the browser account checks behind `verify google` and `verify tiktok`: how the
// ABOUTME: page a profile lands on maps to active, expired, restricted or escalated.

import { test } from "node:test";
import assert from "node:assert/strict";

import { googleStatusFromUrl } from "../scripts/login.mjs";
import { tiktokStatusFromPage } from "../scripts/tiktok-login.mjs";

test("a profile that reaches the Google account dashboard or setup wizard is active", () => {
  assert.equal(googleStatusFromUrl("https://myaccount.google.com/?pli=1"), "active");
  assert.equal(googleStatusFromUrl("https://gds.google.com/web/recoveryoptions"), "active");
});

test("a profile Google signs out is expired; a disabled account is restricted; a challenge needs a person", () => {
  assert.equal(googleStatusFromUrl("https://www.google.com/account/about/"), "expired");
  assert.equal(googleStatusFromUrl("https://accounts.google.com/v3/signin/identifier?continue=x"), "expired");
  assert.equal(googleStatusFromUrl("https://accounts.google.com/ServiceLogin?service=accountsettings"), "expired");
  assert.equal(googleStatusFromUrl("https://accounts.google.com/v3/signin/speedbump/disabled?x=1"), "restricted");
  assert.equal(googleStatusFromUrl("https://accounts.google.com/v3/signin/challenge/pwd?x=1"), "escalated");
});

test("a Google page that says nothing about the session is inconclusive", () => {
  assert.throws(() => googleStatusFromUrl("chrome-error://chromewebdata/"), /inconclusive/);
});

test("TikTok: the page names the account → active; no user → expired; ban text → restricted", () => {
  assert.equal(tiktokStatusFromPage({ uniqueId: "Tok_1", banned: false }, "tok_1"), "active");
  assert.equal(tiktokStatusFromPage({ uniqueId: null, banned: false }, "tok_1"), "expired");
  assert.equal(tiktokStatusFromPage({ uniqueId: null, banned: true }, "tok_1"), "restricted");
  assert.throws(() => tiktokStatusFromPage({ uniqueId: "other", banned: false }, "tok_1"), /@other/);
});
