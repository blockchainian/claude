// ABOUTME: Tests for runWithConcurrency — the bounded parallel driver behind --concurrency.
// ABOUTME: Asserts the in-flight cap holds, every item runs, and results keep input order.
import { test } from "node:test";
import assert from "node:assert/strict";
import { runWithConcurrency } from "../scripts/concurrency.mjs";

const tick = (ms = 5) => new Promise((r) => setTimeout(r, ms));

test("runWithConcurrency never exceeds the limit and runs every item", async () => {
  let inFlight = 0;
  let peak = 0;
  const items = Array.from({ length: 20 }, (_, i) => i);
  const results = await runWithConcurrency(items, 4, async (n) => {
    inFlight++;
    peak = Math.max(peak, inFlight);
    await tick();
    inFlight--;
    return n * 2;
  });
  assert.equal(peak, 4, "should saturate the limit but never pass it");
  assert.deepEqual(results, items.map((n) => n * 2), "results follow input order");
});

test("runWithConcurrency with limit 1 is sequential", async () => {
  let inFlight = 0;
  let peak = 0;
  const order = [];
  await runWithConcurrency([1, 2, 3], 1, async (n) => {
    inFlight++;
    peak = Math.max(peak, inFlight);
    await tick();
    order.push(n);
    inFlight--;
  });
  assert.equal(peak, 1, "only one worker at a time");
  assert.deepEqual(order, [1, 2, 3], "processed in order");
});

test("runWithConcurrency caps at the item count when the limit is larger", async () => {
  let inFlight = 0;
  let peak = 0;
  await runWithConcurrency([1, 2], 10, async () => {
    inFlight++;
    peak = Math.max(peak, inFlight);
    await tick();
    inFlight--;
  });
  assert.equal(peak, 2, "no more workers than items");
});

test("runWithConcurrency defaults a bad limit to 1", async () => {
  let peak = 0;
  let inFlight = 0;
  for (const bad of [undefined, 0, -3, NaN]) {
    inFlight = 0;
    peak = 0;
    await runWithConcurrency([1, 2, 3], bad, async () => {
      inFlight++;
      peak = Math.max(peak, inFlight);
      await tick();
      inFlight--;
    });
    assert.equal(peak, 1, `limit ${bad} should fall back to 1`);
  }
});

test("runWithConcurrency returns [] for no items", async () => {
  const results = await runWithConcurrency([], 5, async () => {
    throw new Error("worker must not run");
  });
  assert.deepEqual(results, []);
});
