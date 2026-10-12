// ABOUTME: Tests translate_paper.mjs's checks on real translator answers from the pilots: math kept (repairs and
// ABOUTME: inline-to-display moves allowed, nesting rejected), links kept, and the course glossary's append-only update.

import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import * as tp from "../scripts/translate_paper.mjs";

test("a repaired misread passes the math check", () => {
  const src = "the product \\(\\bar { C } .\\) of \\(n \\times n\\) matrices \\(O ( s ^ { 3 } )\\)";
  const out = "# t\n乘积 \\(C\\)，即 \\(n \\times n\\) 矩阵 \\(O(s^3)\\)";
  assert.equal(tp.checkMath(out, src), null);
});

test("a formula moved from inline to display passes", () => {
  const src = "\\[a\\]\n\nThus \\(Q(m,n,p)=\\Theta(1+mn/\\mathcal{B})\\).";
  const out = "\\[a\\]\n\n因此\n\n\\[Q(m,n,p)=\\Theta(1+mn/\\mathcal{B})\\]";
  assert.equal(tp.checkMath(out, src), null);
});

test("a nested formula fails the math check", () => {
  const src = "or \\(( n \\leq \\alpha )\\) and m, \\(p > \\alpha\\)";
  const out = "或 \\(( n \\leq \\alpha )\\) 且 \\(m, \\(p > \\alpha\\)";
  assert.match(tp.checkMath(out, src), /KaTeX cannot parse/);
});

test("a dropped display equation fails", () => {
  assert.match(tp.checkMath("# t\n只有文字", "\\[x = 1\\]"), /display math: 0 of 1/);
});

test("a lost link target fails", () => {
  const src = "DOI: [10.1145/2071379.2071383](http://doi.acm.org/10.1145/2071379.2071383)";
  assert.equal(tp.checkLinks("DOI：[10.1145/2071379.2071383](http://doi.acm.org/10.1145/2071379.2071383)", src), null);
  assert.match(tp.checkLinks("DOI：10.1145/2071379.2071383", src), /links lost/);
});

test("course glossary path from the source folder", () => {
  const meta = { source_pdf: "/Users/x/Documents/MIT/6.1220-Design and Analysis of Algorithms/L03-divide-and-conquer-fft.pdf" };
  assert.equal(tp.courseGlossaryPath(meta, "/g"), "/g/6.1220.md");
  assert.equal(tp.courseGlossaryPath({ source_pdf: "/papers/Systems/x.pdf" }, "/g"), "/g/Systems.md");
});

test("new terms are appended, known ones and keep-as-is skipped", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "tp-"));
  const file = path.join(dir, "6.1220.md");
  fs.writeFileSync(file, "work: 工作量\n");
  const glossary = "Title: x = y\n\nTerms:\n- Work = 工作量\n- span = 跨度\n- Cilk = keep as is\n";
  assert.equal(tp.appendCourseTerms(file, glossary), 1);
  assert.equal(fs.readFileSync(file, "utf8"), "work: 工作量\nspan: 跨度\n");
  assert.equal(tp.appendCourseTerms(file, glossary), 0);
  fs.rmSync(dir, { recursive: true });
});

test("words MinerU set as math need not come back as formulas", () => {
  const src = "Extensions to \\(\\mathrm { C } { + } { + }\\) such as \\(\\mathrm { C i l k + + \\tilde { s } }\\) runtime";
  assert.equal(tp.checkMath("# t\n对 C++ 的扩展，例如 Cilk++ 的运行时", src), null);
  assert.equal(tp.wordAsMath("\\(\\operatorname { S Q L }\\)"), true);
  assert.equal(tp.wordAsMath("\\(\\mathrm { S Q L / J }\\)"), true);
  assert.equal(tp.wordAsMath("\\(n \\times n\\)"), false);
});

test("math written in dollars fails", () => {
  assert.match(tp.checkMath("# t\n因此 $\\left| S \\right| < 0$。", "so \\(\\left| S \\right| < 0\\)."), /\$\.\.\.\$/);
  assert.equal(tp.checkMath("# t\n花费 \\$10.00", "costs \\$10.00"), null);
});

test("measurements and percentiles set as math need not come back as formulas", () => {
  for (const m of ["\\(\\dot { 9 5 ^ { t h } }\\)", "\\(4 7 5 \\mu \\mathrm { s }\\)", "\\(\\mathrm { p } 7 5\\)"]) assert.equal(tp.wordAsMath(m), true, m);
  assert.equal(tp.wordAsMath("\\(n^2 + 1\\)"), false);
  assert.equal(tp.wordAsMath("\\(O(n)\\)"), false);
});

test("a lone letter or Greek letter need not come back as a formula", () => {
  for (const m of ["\\(\\mu\\)", "\\(\\mathrm{L^4}\\)", "\\(k\\)"]) assert.equal(tp.wordAsMath(m), true, m);
  for (const m of ["\\(64 \\times 64\\)", "\\(k + 1\\)", "\\(O(n^2)\\)"]) assert.equal(tp.wordAsMath(m), false, m);
});

test("a formula written with Unicode superscripts counts as kept", () => {
  assert.equal(tp.plainText("\\(L^{4}\\)"), "L⁴");
  assert.equal(tp.plainText("\\(\\mathbb{R}^*\\)"), "R*");
  assert.equal(tp.checkMath("# t\n在 L⁴ 中，L⁴ 很快", "In \\(L^{4}\\), \\(L^{4}\\) is fast"), null);
  assert.match(tp.checkMath("# t\n在 L 中", "In \\(L^{4} + x\\), \\(L^{4} + y\\) is fast"), /inline math/);
});

test("dollars inside \\text fail, since the renderer escapes them as currency", () => {
  const out = "# t\n\\[\\text {g(n) = $\\Theta$(f(n))}\\]";
  assert.match(tp.checkMath(out, "\\[\\text {g(n) = }\\Theta\\text{(f(n))}\\]"), /KaTeX cannot parse/);
});

test("a dropped slide placeholder is put back when the paragraphs line up", () => {
  const src = "# T\n\nTo switch processes, save the state.\n\n⟦IMG:p7-0-0⟧\n\nHere is a sketch.\n";
  assert.equal(tp.restoreTokens("# 标题\n\n要切换进程，需保存状态。\n\n这里是示意。\n", src),
    "# 标题\n\n要切换进程，需保存状态。\n\n⟦IMG:p7-0-0⟧\n\n这里是示意。\n");
  const merged = "# 标题\n\n要切换进程，需保存状态。这里是示意。\n";
  assert.equal(tp.restoreTokens(merged, src), merged);
});

test("a procedure name with a subscript written as text counts as kept", () => {
  const src = "runs \\(\\mathsf { T R A N S F E R } _ { 1 }\\) and \\(\\mathsf { T R A N S F E R } _ { \\mathsf { 2 } }\\)";
  assert.equal(tp.checkMath("# t\n运行 TRANSFER₁ 和 TRANSFER<sub>2</sub>", src), null);
});

test("lost placeholders are restored stretch by stretch around the ones still there", () => {
  const src = "# T\n\nA.\n\n⟦CODE:c1⟧\n\nB.\n\n⟦CODE:c2⟧\n\nC. D.\n\n⟦CODE:c3⟧\n\nE.\n";
  // c1 lost in a stretch that lines up; c3 lost where the translator merged two paragraphs: left alone
  const out = "# 标\n\n甲。\n\n乙。\n\n⟦CODE:c2⟧\n\n丙。丁。戊。\n";
  assert.equal(tp.restoreTokens(out, src), "# 标\n\n甲。\n\n⟦CODE:c1⟧\n\n乙。\n\n⟦CODE:c2⟧\n\n丙。丁。戊。\n");
});

test("a token the translator replaced by its own paragraph is put back", () => {
  const src = "# T\n\n### Assistant\n\n⟦CODE:c1⟧\n\n### User\n\nHi.\n";
  assert.equal(tp.restoreTokens("# 标\n\n### 助手\n\n（代码）\n\n### 用户\n\n你好。\n", src),
    "# 标\n\n### 助手\n\n⟦CODE:c1⟧\n\n### 用户\n\n你好。\n");
});

test("a superscript the translator turned into <sup> goes back unless it is a footnote mark", () => {
  const src = "### 4.2 L⁴Linux — Design\n\nsee note<sup>3</sup>.";
  assert.equal(tp.unmarkExponents("### 4.2 L<sup>4</sup>Linux — 设计\n\n见注<sup>3</sup>。", src), "### 4.2 L⁴Linux — 设计\n\n见注<sup>3</sup>。");
});
