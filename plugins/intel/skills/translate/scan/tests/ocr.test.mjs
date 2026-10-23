// ABOUTME: Tests the OCR page check that catches a formula left unclosed, as on 6.1220 L18 page 2.
import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { mathBalanced } from "../scripts/ocr.mjs";

test("an unclosed inline formula fails the check", () => {
  assert.equal(mathBalanced("all edges: \\(\\forall e\\in E\\ \\exists v\\in S\\ v\\in e  \n**Parameter**: \\(k\\)"), false);
});

test("closed inline and display formulas pass", () => {
  assert.equal(mathBalanced("\\(k\\) and\n\\[\n\\sum_i x_i\n\\]\n"), true);
});

test("literal TeX delimiters in the real page 135 listing are not math", () => {
  const listing = readFileSync(new URL("./fixtures/literate-programming-p135.md", import.meta.url), "utf8");
  assert.match(listing, /\\\]/);
  assert.match(listing, /\\\[5/);
  assert.equal(mathBalanced("```tex\n" + listing + "\n```\n"), true);
});

test("inline code delimiters do not open or close math", () => {
  assert.equal(mathBalanced("Use `\\]` or `\\[5]` literally, and \\(x\\) as math."), true);
  assert.equal(mathBalanced("\\(x `\\)`"), false);
  assert.equal(mathBalanced("```tex\n\\[5]\n```\n\\(x"), false);
});
