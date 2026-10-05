// ABOUTME: Tests page helpers against fake locators with real timeout errors.
// ABOUTME: Checks click diagnostics and preserves boolean and error behavior.

import { test } from "node:test";
import assert from "node:assert/strict";
import { clickFirst } from "../scripts/page-helpers.mjs";

function pageWithClicks(click) {
  return { getByText: (text, options) => {
    assert.deepEqual(options, { exact: false });
    return { first: () => ({ click: ({ timeout }) => click(text, timeout) }) };
  } };
}

function timeout(message) {
  return Object.assign(new Error(message), { name: "TimeoutError" });
}

test("clickFirst records trimmed timeout call logs before trying the next text", async () => {
  const misses = [];
  const reason = 'locator.click: Timeout 123ms exceeded.\nCall log:\n<div> intercepts pointer events';
  const page = pageWithClicks(async (text, ms) => {
    assert.equal(ms, 123);
    if (text === "Login") throw timeout(`  ${reason}  `);
  });
  assert.equal(await clickFirst(page, ["Login", "Sign in"], 123, misses), true);
  assert.deepEqual(misses, [{ text: "Login", reason }]);
});

test("clickFirst records every miss, caps reasons, and still returns false", async () => {
  const misses = [];
  const page = pageWithClicks(async () => { throw timeout(" x".repeat(2000)); });
  assert.equal(await clickFirst(page, ["Login", "Sign in"], undefined, misses), false);
  assert.deepEqual(misses.map(({ text }) => text), ["Login", "Sign in"]);
  assert.equal(misses[0].reason.length, 2000);
  assert.equal(await clickFirst(page, ["Login"]), false);
});

test("clickFirst rethrows non-timeout errors without recording a miss", async () => {
  const error = new Error("page closed");
  const misses = [];
  await assert.rejects(clickFirst(pageWithClicks(async () => { throw error; }), ["Login"], 1, misses), (e) => e === error);
  assert.deepEqual(misses, []);
});
