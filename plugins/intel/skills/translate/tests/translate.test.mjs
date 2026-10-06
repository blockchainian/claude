// ABOUTME: Tests translate.mjs: the prompt and rules, answer checks, the private CODEX_HOME layout,
// ABOUTME: the codex exec command line, and (with TRANSLATE_E2E=1) one real gpt-6-luna call.

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import * as tr from "../scripts/translate.mjs";

const scriptsDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "scripts");
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "translate-test-"));

describe("prompt and rules", () => {
  const meta = { title: "Traction", author: "Weinberg" };
  const sec = { id: "04", title: "Traction Channels", label: "第一章", kind: "chapter", words: 10, file: "text/04-x.txt" };
  const prompt = tr.buildPrompt(meta, sec, "Hello world.", "traction → 牵引力");

  test("prompt names book, section and glossary", () => {
    assert.ok(prompt.includes("Book: Traction by Weinberg"));
    assert.ok(prompt.includes("第一章 Traction Channels"));
    assert.ok(prompt.includes("traction → 牵引力"));
    assert.ok(prompt.trimEnd().endsWith("Hello world."));
  });

  test("rules forbid tools and demand the title line", () => {
    assert.ok(tr.RULES.includes("do not run commands") && tr.RULES.includes('"# "'));
  });

  test("rules tell the model to keep image placeholders", () => {
    assert.ok(tr.RULES.includes("⟦IMG:") && tr.RULES.includes("Copy every"));
  });

  test("epub rules carry the LaTeX conventions with single backslashes", () => {
    assert.ok(tr.RULES_EPUB.includes("\\(\\mathbf{x}\\)") && tr.RULES_EPUB.includes("\\[ ... \\tag{9.3} \\]"));
    assert.ok(!tr.RULES_EPUB.includes("\\\\("));
  });
});

describe("answer checks", () => {
  test("answer without title rejected", () => {
    assert.notEqual(tr.checkOutput("正文而已", 5), null);
  });

  test("short answer rejected", () => {
    assert.notEqual(tr.checkOutput("# 标题\n\n短", 100), null);
  });

  test("good answer accepted", () => {
    assert.equal(tr.checkOutput("# 标题\n\n" + "汉".repeat(200), 100), null);
  });
});

describe("codex home and command", () => {
  const fakeHome = path.join(tmp, "codex-src");
  fs.mkdirSync(fakeHome, { recursive: true });
  fs.writeFileSync(path.join(fakeHome, "auth.json"), '{"tok":"x"}');
  const saved = process.env.CODEX_HOME;
  process.env.CODEX_HOME = fakeHome;
  const home = tr.codexHome(path.join(tmp, "a"), "gpt-6-luna", "low", "/x/instructions.md", "priority");
  const cfg = fs.readFileSync(path.join(home, "config.toml"), "utf8");
  const std = tr.codexHome(path.join(tmp, "b"), "gpt-6-luna", "low", "/x/i.md", "standard");
  const stdCfg = fs.readFileSync(path.join(std, "config.toml"), "utf8");
  if (saved === undefined) delete process.env.CODEX_HOME; else process.env.CODEX_HOME = saved;

  test("codex config pins model, effort, tier", () => {
    assert.ok(cfg.includes('model = "gpt-6-luna"') && cfg.includes('model_reasoning_effort = "low"'));
    assert.ok(cfg.includes('service_tier = "priority"') && cfg.includes("project_doc_max_bytes = 0"));
    assert.equal(fs.readFileSync(path.join(home, "auth.json"), "utf8"), '{"tok":"x"}');
  });

  test("standard tier omits service_tier", () => {
    assert.ok(!stdCfg.includes("service_tier"));
  });

  test("codex exec runs read-only, ephemeral, with every tool off and the prompt on stdin", () => {
    const args = tr.codexArgs("/w", "/w/last.txt");
    assert.deepEqual(args.slice(0, 8), ["exec", "--ignore-rules", "--skip-git-repo-check", "--ephemeral", "-C", "/w", "-s", "read-only"]);
    for (const f of tr.OFF) assert.ok(args.includes(f), f);
    assert.deepEqual(args.slice(-4), ["--json", "-o", "/w/last.txt", "-"]);
  });
});

describe("cli", () => {
  const work = path.join(tmp, "work");
  fs.mkdirSync(path.join(work, "text"), { recursive: true });
  fs.mkdirSync(path.join(work, "translated"), { recursive: true });
  fs.writeFileSync(path.join(work, "text", "01-one.xhtml"), "<p>one</p>");
  fs.writeFileSync(path.join(work, "text", "02-two.xhtml"), "<p>two</p>");
  fs.writeFileSync(path.join(work, "translated", "01-one.md"), "# 一\n\n正文\n");
  fs.writeFileSync(path.join(work, "sections.json"), JSON.stringify({
    title: "T", author: "A", source_kind: "epub",
    sections: [{ id: "01", title: "One", label: "第一章", kind: "chapter", words: 1, file: "text/01-one.xhtml" },
      { id: "02", title: "Two", label: "第二章", kind: "chapter", words: 1, file: "text/02-two.xhtml" }],
  }));

  test("--dry-run prints the epub rules and the first untranslated section's prompt", () => {
    const r = spawnSync(process.execPath, [path.join(scriptsDir, "translate.mjs"), work, "--dry-run"], { encoding: "utf8" });
    assert.equal(r.status, 0, r.stderr);
    assert.ok(r.stdout.startsWith(tr.RULES_EPUB + "\n-----\nBook: T by A\nSection: 第二章 Two (chapter, 1 words)"), r.stdout.slice(0, 200));
  });

  test("a missing codex is reported per section, not fatal", () => {
    const r = spawnSync(process.execPath, [path.join(scriptsDir, "translate.mjs"), work, "--only", "02"],
      { encoding: "utf8", env: { ...process.env, PATH: path.join(tmp, "empty-bin"), CODEX_HOME: path.join(tmp, "codex-src") } });
    assert.equal(r.status, 1);
    assert.ok(r.stdout.includes("1 of 1 sections to translate with gpt-6-luna/low/priority, 20 in flight"), r.stdout);
    assert.ok(r.stdout.includes("FAILED 02: ") && r.stdout.includes("done: 0/1 sections translated, 1 failed"), r.stdout);
  });

  test("argparse-style errors exit 2", () => {
    const r = spawnSync(process.execPath, [path.join(scriptsDir, "translate.mjs"), work, "--jobs", "x"], { encoding: "utf8" });
    assert.equal(r.status, 2);
    assert.ok(r.stderr.includes("argument --jobs: invalid int value: 'x'"), r.stderr);
  });
});

test("luna returns a titled Chinese translation (e2e)", { skip: !process.env.TRANSLATE_E2E && "set TRANSLATE_E2E=1 to spend one gpt-6-luna call" }, async () => {
  const [md, usage, seconds] = await tr.runCodex("Book: Test\nSection: Preface (front, 12 words)\n\nSource text:\n\nTraction is a sign that your company is taking off. Nothing else matters.\n",
    "gpt-6-luna", "low", "priority");
  assert.equal(tr.checkOutput(md, 12), null, md.slice(0, 200));
  assert.ok(md.includes("牵引力"), md.slice(0, 200));
  assert.ok(usage && usage.output_tokens > 0 && seconds >= 0);
});

test("answer preserves placeholder multiplicities and numeric sup marker set", () => {
  const src = '<p>⟦IMG:e1⟧ ⟦IMG:e1⟧ ⟦IMG:e2⟧ <sup class="note">12</sup><sup>21</sup></p>';
  const good = '# 标题\n\n正文 ⟦IMG:e2⟧ ⟦IMG:e1⟧ ⟦IMG:e1⟧ <sup>21</sup><sup>12</sup>';
  assert.equal(tr.checkOutput(good, 1, src), null);
  assert.match(tr.checkOutput(good.replace('⟦IMG:e1⟧ ', ''), 1, src), /missing.*⟦IMG:e1⟧/);
  assert.match(tr.checkOutput(good + ' ⟦IMG:e2⟧', 1, src), /extra.*⟦IMG:e2⟧/);
  assert.match(tr.checkOutput(good.replace('<sup>12</sup>', ''), 1, src), /missing.*<sup>12<\/sup>/);
  assert.match(tr.checkOutput(good + '<sup>30</sup>', 1, src), /extra.*<sup>30<\/sup>/);
  assert.equal(tr.checkOutput(good + '<sup>12</sup>', 1, src), null);
});
