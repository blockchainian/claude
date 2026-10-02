// ABOUTME: Tests the request blocklist matcher: host wildcards and resource types.
// ABOUTME: No browser; installBlocklist is exercised live.

import { test } from "node:test";
import assert from "node:assert/strict";

import { isBlockedHost, shouldBlock, isLoginAssetHost, configureBlocklist } from "../scripts/traffic.mjs";

configureBlocklist([{ blockedHosts: ["cluster*.beta.trade", "pulse*.beta.trade", "friends.beta.trade", "telemetry.beta.trade", "beta-assets-v2.beta-cdn.io", "app-actions*.alpha.family", "mobula-api.alpha.family"] }]);

test("host wildcards match the blackholed hosts and nothing else", () => {
  assert.equal(isBlockedHost("cluster7.beta.trade"), true);
  assert.equal(isBlockedHost("pulse-v2.beta.trade"), true);
  assert.equal(isBlockedHost("friends.beta.trade"), true);
  assert.equal(isBlockedHost("telemetry.beta.trade"), true);
  assert.equal(isBlockedHost("beta-assets-v2.beta-cdn.io"), true);
  assert.equal(isBlockedHost("app-actions-eu.alpha.family"), true);
  assert.equal(isBlockedHost("mobula-api.alpha.family"), true);
  assert.equal(isBlockedHost("beta.trade"), false);
  assert.equal(isBlockedHost("api6.beta.trade"), false);
  assert.equal(isBlockedHost("alpha.family"), false);
  assert.equal(isBlockedHost("prod-api.alpha.family"), false);
  assert.equal(isBlockedHost("accounts.google.com"), false);
  assert.equal(isBlockedHost("evil-cluster7.beta.trade.example.com"), false);
});

test("isLoginAssetHost is true for Google/gstatic hosts and nothing else", () => {
  assert.equal(isLoginAssetHost("www.google.com"), true);
  assert.equal(isLoginAssetHost("accounts.google.com"), true);
  assert.equal(isLoginAssetHost("www.gstatic.com"), true);
  assert.equal(isLoginAssetHost("alpha.family"), false);
  assert.equal(isLoginAssetHost("mobula-api.alpha.family"), false);
  assert.equal(isLoginAssetHost("notgoogle.com"), false);
  assert.equal(isLoginAssetHost("google.com.evil.example"), false);
});

test("shouldBlock drops app images/media/fonts but lets Google sign-in assets through", () => {
  assert.equal(shouldBlock("https://alpha.family/font.woff2", "font"), true);
  assert.equal(shouldBlock("https://gamma.ai/clip.mp4", "media"), true);
  assert.equal(shouldBlock("https://www.google.com/recaptcha/api2/payload/tile.png", "image"), false);
  assert.equal(shouldBlock("https://www.gstatic.com/recaptcha/pic.png", "image"), false);
  assert.equal(shouldBlock("https://accounts.google.com/v3/signin", "document"), false);
  assert.equal(shouldBlock("https://challenges.cloudflare.com/turnstile.js", "script"), false);
  assert.equal(shouldBlock("https://prod-api.alpha.family/v2/users/current", "fetch"), false);
  assert.equal(shouldBlock("https://cluster7.beta.trade/api", "fetch"), true);
  // A blackholed host is dropped even for an image from Google-lookalike data hosts.
  assert.equal(shouldBlock("https://mobula-api.alpha.family/chart.png", "image"), true);
  assert.equal(shouldBlock("not a url", "fetch"), false);
});
