// ABOUTME: Tests the pure OTP-extraction helper behind the beta email signup flow.
// ABOUTME: The IMAP poller itself is verified live.

import { test } from "node:test";
import assert from "node:assert/strict";

import { extractOtp, otpCandidates } from "../scripts/email-otp.mjs";

test("six digits near 'code'", () => {
  assert.equal(extractOtp("Your beta verification code is 481920. It expires in 10 minutes."), "481920");
});

test("prefers the run nearest a code word", () => {
  // A stray 4-digit year and a Ray-ID-like number must not win over the real code.
  assert.equal(extractOtp("beta 2026. Ray ID a4b. Enter code 730514 to continue. Footer 12345678."), "730514");
});

test("six digits without a keyword", () => {
  assert.equal(extractOtp("Confirm: 246810"), "246810");
});

test("null when there are no digits", () => {
  assert.equal(extractOtp("no numbers here"), null);
  assert.equal(extractOtp(""), null);
});

test("ignores long digit runs", () => {
  assert.equal(extractOtp("Order 1234567890123 shipped"), null);
});

test("otpCandidates lists every short run", () => {
  const cands = otpCandidates("code 111111 or 222222, year 2026");
  assert.ok(cands.includes("111111"));
  assert.ok(cands.includes("222222"));
});
