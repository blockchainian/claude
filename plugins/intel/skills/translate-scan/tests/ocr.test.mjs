// ABOUTME: Tests the OCR page check that catches a formula left unclosed, as on 6.1220 L18 page 2.
import assert from "node:assert/strict";
import test from "node:test";
import { mathBalanced } from "../scripts/ocr.mjs";

test("an unclosed inline formula fails the check", () => {
  assert.equal(mathBalanced("all edges: \\(\\forall e\\in E\\ \\exists v\\in S\\ v\\in e  \n**Parameter**: \\(k\\)"), false);
});

test("closed inline and display formulas pass", () => {
  assert.equal(mathBalanced("\\(k\\) and\n\\[\n\\sum_i x_i\n\\]\n"), true);
});
