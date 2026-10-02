// ABOUTME: Tests captcha.mjs's pure helpers: the image-to-text task builder and the grid-ready check.
// ABOUTME: The network call (createTask) is not covered — it costs money and needs a key.

import { test } from "node:test";
import assert from "node:assert/strict";

import { buildImageToTextTask, gridReady } from "../scripts/captcha.mjs";

test("buildImageToTextTask carries the base64 body", () => {
  assert.deepEqual(buildImageToTextTask("AAAA"), { type: "ImageToText", module: "common", body: "AAAA" });
  assert.throws(() => buildImageToTextTask(""));
});

test("gridReady is true only when every tile is loaded, decoded, and fully faded in", () => {
  const ok = (over) => ({ hasImg: true, complete: true, naturalWidth: 100, opacity: 1, ...over });
  assert.equal(gridReady([ok(), ok(), ok()]), true);
  // still downloading / grainy
  assert.equal(gridReady([ok(), ok({ complete: false })]), false);
  // decoded to zero pixels (blank tile)
  assert.equal(gridReady([ok(), ok({ naturalWidth: 0 })]), false);
  // mid fade-in after a dynamic swap
  assert.equal(gridReady([ok(), ok({ opacity: 0.4 })]), false);
  // a cell caught mid-swap with no image at all
  assert.equal(gridReady([ok(), ok({ hasImg: false })]), false);
  // near-1 opacity from a nearly-finished transition still counts
  assert.equal(gridReady([ok({ opacity: 0.995 })]), true);
});

test("gridReady is false when there is no grid to judge", () => {
  assert.equal(gridReady([]), false);
  assert.equal(gridReady(null), false);
});
