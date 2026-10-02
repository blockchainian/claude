// ABOUTME: Tests classify-image.mjs's pure output parser and prompt builder.
// ABOUTME: The codex exec call itself is not covered — it costs model quota and needs a login.

import { test } from "node:test";
import assert from "node:assert/strict";

import { parseCells, buildPrompt, parseText } from "../scripts/classify-image.mjs";

test("parseCells keeps only valid in-range cell indices, de-duplicated and sorted", () => {
  assert.deepEqual(parseCells('{"cells":[7,2,3]}', 3), [2, 3, 7]);
  assert.deepEqual(parseCells('{"cells":[]}', 3), []);
  // out-of-range for a 3x3 (max index 8) and non-integers are dropped
  assert.deepEqual(parseCells('{"cells":[9,-1,2.5,"x",4]}', 3), [4]);
  // 4x4 allows up to 15
  assert.deepEqual(parseCells('{"cells":[15,5,15]}', 4), [5, 15]);
});

test("parseText returns the transcribed CAPTCHA characters with whitespace stripped", () => {
  assert.equal(parseText('{"text":"aB3xY"}'), "aB3xY");
  assert.equal(parseText('{"text":" a b c "}'), "abc");
  assert.equal(parseText('{"text":""}'), "");
});

test("parseText is empty on malformed or unexpected output", () => {
  assert.equal(parseText("not json"), "");
  assert.equal(parseText('{"foo":1}'), "");
  assert.equal(parseText('{"text":123}'), "");
});

test("parseCells is empty on malformed or unexpected output", () => {
  assert.deepEqual(parseCells("not json", 3), []);
  assert.deepEqual(parseCells('{"foo":1}', 3), []);
  assert.deepEqual(parseCells('{"cells":"2,3"}', 3), []);
  assert.deepEqual(parseCells("", 4), []);
});

test("buildPrompt states the grid size, the object, and the precision rules", () => {
  const p = buildPrompt("a bus", 3);
  assert.match(p, /3x3 grid of 9/);
  assert.match(p, /row0: 0,1,2/);
  assert.match(p, /row2: 6,7,8/);
  assert.match(p, /a bus/);
  assert.match(p, /empty list/);
  assert.match(p, /part of the object extends into it/); // edge cells still count
  assert.match(p, /do not guess/); // precision guard against false positives
  const p4 = buildPrompt("bicycles", 4);
  assert.match(p4, /4x4 grid of 16/);
  assert.match(p4, /row3: 12,13,14,15/);
});
