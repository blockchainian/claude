// ABOUTME: Tests sms-otp.mjs's pure response parsers and the price filter for HeroSMS.
// ABOUTME: The network calls (rent a number, poll for the code) are not covered — they cost money.

import { test } from "node:test";
import assert from "node:assert/strict";

import { parseBalance, parseNumber, parseStatus, affordableCountries, backoffDelays } from "../scripts/sms-otp.mjs";

test("backoffDelays doubles from 1s and stays within a ~1min budget", () => {
  // Retry a flapping HeroSMS request 1s, 2s, 4s, … apart, never exceeding the total budget.
  assert.deepEqual(backoffDelays(60000, 1000), [1000, 2000, 4000, 8000, 16000, 32000]);
});

test("backoffDelays stops early under a smaller budget", () => {
  assert.deepEqual(backoffDelays(7000, 1000), [1000, 2000, 4000]);
  assert.deepEqual(backoffDelays(500, 1000), []);
});

test("parseBalance reads the amount", () => {
  assert.equal(parseBalance("ACCESS_BALANCE:12.34"), 12.34);
  assert.equal(parseBalance("ACCESS_BALANCE:0.5"), 0.5);
});

test("parseBalance is null on an error token or empty", () => {
  assert.equal(parseBalance("BAD_KEY"), null);
  assert.equal(parseBalance(""), null);
  assert.equal(parseBalance(null), null);
});

test("parseNumber splits the activation id and phone", () => {
  assert.deepEqual(parseNumber("ACCESS_NUMBER:123456789:79001234567"), {
    activationId: "123456789",
    phone: "79001234567",
  });
});

test("parseNumber is null when no number was granted", () => {
  assert.equal(parseNumber("NO_NUMBERS"), null);
  assert.equal(parseNumber(""), null);
});

test("parseStatus reads a delivered code", () => {
  assert.deepEqual(parseStatus("STATUS_OK:456123"), { state: "ok", code: "456123" });
});

test("parseStatus treats every wait variant as waiting", () => {
  assert.deepEqual(parseStatus("STATUS_WAIT_CODE"), { state: "waiting" });
  assert.deepEqual(parseStatus("STATUS_WAIT_RESEND"), { state: "waiting" });
  assert.deepEqual(parseStatus("STATUS_WAIT_RETRY:999888"), { state: "waiting", lastCode: "999888" });
});

test("parseStatus reads cancellation and an unknown reply", () => {
  assert.deepEqual(parseStatus("STATUS_CANCEL"), { state: "cancelled" });
  assert.deepEqual(parseStatus("weird"), { state: "unknown", raw: "weird" });
});

test("affordableCountries keeps in-stock countries at or under the price, cheapest first", () => {
  const prices = {
    "0": { go: { cost: 0.08, count: 500 } }, // too dear
    "6": { go: { cost: 0.04, count: 120 } },
    "10": { go: { cost: 0.02, count: 0 } }, // out of stock
    "12": { go: { cost: 0.05, count: 30 }, wa: { cost: 0.01, count: 9 } },
    "15": { wa: { cost: 0.01, count: 9 } }, // other service only
  };
  assert.deepEqual(affordableCountries(prices, "go", 0.05), [
    { country: "6", cost: 0.04, count: 120 },
    { country: "12", cost: 0.05, count: 30 },
  ]);
});

test("affordableCountries drops blacklisted countries whatever their price", () => {
  const prices = {
    "41": { go: { cost: 0.01, count: 500 } }, // cheapest but blacklisted
    "6": { go: { cost: 0.02, count: 500 } }, // blacklisted
    "4": { go: { cost: 0.03, count: 120 } },
    "36": { go: { cost: 0.05, count: 30 } },
  };
  assert.deepEqual(affordableCountries(prices, "go", 0.1, [41, 6]), [
    { country: "4", cost: 0.03, count: 120 },
    { country: "36", cost: 0.05, count: 30 },
  ]);
  // A blacklist given as strings works the same.
  assert.deepEqual(affordableCountries(prices, "go", 0.1, ["41", "6"]).map((c) => c.country), ["4", "36"]);
});

test("affordableCountries is empty when nothing qualifies", () => {
  assert.deepEqual(affordableCountries({}, "go", 0.05), []);
  assert.deepEqual(affordableCountries({ "0": { go: { cost: 1, count: 5 } } }, "go", 0.05), []);
});
