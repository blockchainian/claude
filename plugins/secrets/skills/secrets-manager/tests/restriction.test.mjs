// ABOUTME: Tests the pure JWT/decision helpers behind the alpha check-restricted command.
// ABOUTME: No browser and no network.

import { test } from "node:test";
import assert from "node:assert/strict";

import { jwtExp, tokenLive, unwrapJsonQuoted, storedTokenLive, restrictionDecision } from "../scripts/restriction.mjs";

// A signature-less JWT whose payload is `payload` (header + body only).
function jwt(payload) {
  const seg = (obj) => Buffer.from(JSON.stringify(obj)).toString("base64url");
  return `${seg({ alg: "HS256" })}.${seg(payload)}.sig`;
}

test("jwtExp reads exp", () => {
  assert.equal(jwtExp(jwt({ exp: 1234567890 })), 1234567890);
});

test("jwtExp is null without exp or on garbage", () => {
  assert.equal(jwtExp(jwt({ sub: "x" })), null);
  assert.equal(jwtExp("not-a-jwt"), null);
  assert.equal(jwtExp(""), null);
  assert.equal(jwtExp("only.two"), null);
});

test("tokenLive respects the skew", () => {
  assert.equal(tokenLive(jwt({ exp: 1120 }), 1000), true);
  assert.equal(tokenLive(jwt({ exp: 1030 }), 1000), false);
  assert.equal(tokenLive(jwt({ sub: "x" }), 1000), false);
  assert.equal(tokenLive("", 1000), false);
});

test("unwrapJsonQuoted", () => {
  assert.equal(unwrapJsonQuoted('"abc"'), "abc");
  assert.equal(unwrapJsonQuoted("plain"), "plain");
  assert.equal(unwrapJsonQuoted(null), null);
  assert.equal(unwrapJsonQuoted(""), null);
});

test("storedTokenLive unwraps then checks", () => {
  assert.equal(storedTokenLive(`"${jwt({ exp: 1120 })}"`, 1000), true);
  assert.equal(storedTokenLive(`"${jwt({ exp: 1000 })}"`, 1000), false);
  assert.equal(storedTokenLive(null, 1000), false);
});

test("restrictionDecision buckets", () => {
  assert.equal(restrictionDecision(200, { isRestricted: true })[0], "restricted");
  assert.equal(restrictionDecision(200, { isRestricted: false })[0], "ok");
  assert.equal(restrictionDecision(200, { id: "u1" })[0], "ok"); // never synthesize the flag
  assert.equal(restrictionDecision(401, { message: "JWT" })[0], "dead_token");
  assert.equal(restrictionDecision(403, { message: "Forbidden" })[0], "forbidden");
  assert.equal(restrictionDecision(404, null)[0], "error");
  assert.equal(restrictionDecision(502, null)[0], "error");
  assert.deepEqual(restrictionDecision(null, null), ["error", "no response"]);
});
