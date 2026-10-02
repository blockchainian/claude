// ABOUTME: Tests TOTP against pinned vectors and the 30-second window boundary.
// ABOUTME: Deterministic — a fixed secret at a fixed time, no network.

import { test } from "node:test";
import assert from "node:assert/strict";

import { computeTotp } from "../scripts/totp.mjs";

const SECRET = "JBSWY3DPEHPK3PXP";

test("pinned vectors", () => {
  assert.equal(computeTotp(SECRET, 1111111111), "358462");
  assert.equal(computeTotp(SECRET, 59), "996554");
});

test("RFC 6238 SHA1 vector", () => {
  assert.equal(computeTotp("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ", 59), "287082");
});

test("same 30s window gives the same code", () => {
  assert.equal(computeTotp(SECRET, 60), computeTotp(SECRET, 89));
});

test("next window gives a different code", () => {
  assert.notEqual(computeTotp(SECRET, 60), computeTotp(SECRET, 90));
});
