// ABOUTME: Tests the OCR page check that catches a formula left unclosed, as on 6.1220 L18 page 2.
import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import path from "node:path";
import { mathBalanced, PROMPTS, codexWithImage, OCR_DEFAULTS } from "../scripts/ocr.mjs";

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

test("typeset instructions fence the real TeX source listing instead of interpreting its delimiters", () => {
  const listing = readFileSync(new URL("./fixtures/literate-programming-p135.md", import.meta.url), "utf8");
  assert.match(listing, /\\input webmac/);
  assert.match(PROMPTS.typeset, /monospaced/);
  assert.match(PROMPTS.typeset, /TeX\/LaTeX source/);
  assert.ok(PROMPTS.typeset.includes("\\]"));
  assert.ok(PROMPTS.typeset.includes("\\[5]"));
  assert.match(PROMPTS.typeset, /caption outside/);
});

test("the real typeset source page is fenced and math-balanced in three independent OCR calls", {
  skip: !process.env.OCR_CODE_E2E_IMAGE && "set OCR_CODE_E2E_IMAGE to the real source-listing page",
  timeout: 180_000,
}, async () => {
  const listing = readFileSync(new URL("./fixtures/literate-programming-p135.md", import.meta.url), "utf8");
  const commands = [...new Set(listing.match(/\\(?:input|font|inx|fin|con)\b/g))];
  assert.equal(commands.length, 5);
  const out = process.env.OCR_CODE_E2E_OUT;
  if (!out) throw new Error("OCR_CODE_E2E_OUT must name an independent output directory.");
  mkdirSync(out, { recursive: true });
  const results = await Promise.all([1, 2, 3].map(async trial => {
    const started = performance.now();
    const md = await codexWithImage(PROMPTS.typeset, process.env.OCR_CODE_E2E_IMAGE, {
      ...OCR_DEFAULTS, timeout: 120, events: path.join(out, `${trial}.events.jsonl`),
    });
    writeFileSync(path.join(out, `${trial}.md`), md);
    const blocks = [...md.matchAll(/^```[^\n]*\n([\s\S]*?)^```[ \t]*$/gm)].map(match => match[1]);
    const code = blocks.join("\n");
    const result = { trial, mathBalanced: mathBalanced(md), codeBlocks: blocks.length,
      listingInsideCode: commands.every(command => code.includes(command))
        && /\\\]/.test(code) && /\\\[5/.test(code), seconds: (performance.now() - started) / 1000 };
    writeFileSync(path.join(out, `${trial}.json`), JSON.stringify(result, null, 2));
    return result;
  }));
  for (const result of results) {
    assert.equal(result.mathBalanced, true, JSON.stringify(result));
    assert.equal(result.listingInsideCode, true, JSON.stringify(result));
    assert.ok(result.codeBlocks > 0, JSON.stringify(result));
  }
});
