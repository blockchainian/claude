// ABOUTME: Tests translate_segments.mjs: the answer checks (kept strings, broken or added TeX), batching and answer
// ABOUTME: parsing, and (with TRANSLATE_E2E=1) one real codex call on a real segment.

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import * as ts from "../scripts/translate_segments.mjs";

const scripts = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "scripts");

describe("problemWith", () => {
  const code = { text: 'from <code>g</code> to <code>f</code>', keep: ["<code>g</code>", "<code>f</code>"] };
  test("a dropped code element is caught", () => {
    assert.match(ts.problemWith(code, "从 <code>g</code> 开始"), /lost/);
    assert.equal(ts.problemWith(code, "从 <code>g</code> 到 <code>f</code>"), null);
  });
  test("a repeated keep string needs to appear once", () => {
    const seg = { text: "$N$ messages ... $N$", keep: ["k=\"0\"", "k=\"0\""] };
    assert.equal(ts.problemWith(seg, '<a k="0"></a>消息'), null);
  });
  const math = { text: "string of 1's represents ${2^k}-1$, the $k$-bit max", keep: [], math: ["${2^k}-1$", "$k$"], dollar: true };
  test("a lost closing $ is broken math", () => {
    assert.match(ts.problemWith(math, "全为 1 的位串表示 ${2^k}-1，这是 $k$ 位的最大值"), /broken math/);
  });
  test("a reworded formula is fine", () => {
    assert.equal(ts.problemWith(math, "全为 1 的位串表示 ${2^k}-1$，即第 $k$ 位最大值"), null);
  });
  test("TeX added to plain text is caught, money is not", () => {
    const plain = { text: "assumes 2f+1 of 3f+1 replicas", keep: [] };
    assert.match(ts.problemWith(plain, "假定 $3f+1$ 个副本"), /added math/);
    const money = { text: "starts with $200 and ends with $200", keep: [] };
    assert.equal(ts.problemWith(money, "以 $200 开始，以 $200 结束"), null);
  });
  test("a page without $ config ignores dollar pairs", () => {
    const seg = { text: "x", keep: [], math: ["\\(x\\)"], dollar: false };
    assert.equal(ts.problemWith(seg, "余额 $200，然后 $300，\\(x\\)"), null);
  });
  test("an empty answer is missing", () => assert.equal(ts.problemWith(code, ""), "missing"));
});

describe("batches and answers", () => {
  test("consecutive segments split by English word budget", () => {
    const segs = [{ text: "one two three" }, { text: "four five" }, { text: "six" }];
    assert.deepEqual(ts.batches(segs, 4).map((b) => b.length), [1, 2]);
  });
  test("an answer in a code fence parses", () => {
    assert.deepEqual(ts.parseAnswer('```json\n{"u1": "你好"}\n```'), { u1: "你好" });
  });
  test("rules carry the document context", () => assert.match(ts.rules("MIT course notes"), /MIT course notes/));
});

test("real codex call on one segment", { skip: process.env.TRANSLATE_E2E !== "1" }, () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "translate-pdf-test-"));
  const segs = [{ id: "u1", text: "In <code>exec.c</code>, the kernel loads a program.", keep: ["<code>exec.c</code>"], math: [] }];
  fs.writeFileSync(path.join(dir, "segs.json"), JSON.stringify(segs));
  const r = spawnSync("node", [path.join(scripts, "translate_segments.mjs"), path.join(dir, "segs.json"), path.join(dir, "zh.json"),
    "--format", "html"], { encoding: "utf8", timeout: 300000 });
  assert.equal(r.status, 0, r.stdout + r.stderr);
  const zh = JSON.parse(fs.readFileSync(path.join(dir, "zh.json"), "utf8")).u1;
  assert.match(zh, /<code>exec\.c<\/code>/);
  assert.match(zh, /[一-鿿]/);
});
