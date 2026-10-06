#!/usr/bin/env node
// ABOUTME: Translates each extracted section into Chinese Markdown with one no-tool codex exec call per section
// ABOUTME: (gpt-6-luna by default), many sections in flight at once, in a private CODEX_HOME; resumable.
//
// Usage: translate.mjs <work dir> [--only 04,05] [--jobs 20] [--model gpt-6-luna] [--effort low]
//                      [--service-tier priority] [--glossary <file>] [--force] [--dry-run]
// Reads <work>/sections.json; writes <work>/translated/<id>-<slug>.md (+ .events.jsonl); skips sections whose .md exists.
import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { pyDumps, pyStrip } from "./extract.mjs";

export const RULES = `You are a professional literary translator (English -> Simplified Chinese) working on a published
non-fiction book. Everything you need is in the message: do not run commands, do not read or write files.
Translate the whole section, faithfully and fluently, as a Chinese publisher would print it. Never summarize,
never skip a paragraph, never add commentary.

The source is OCR text from a scanned book. Repair it silently: obvious misreads ("Pretace" -> "Preface",
"rnodern" -> "modern", "mineteen" -> "nineteen"), missing spaces in italics ("ofcustomer"), a first
letter that a drop cap detached or lost ("n 2006" -> "In 2006", "Ria we get started" -> "Before we get
started"), and duplicated or stray running heads.

Output Markdown only:
- First line: "# " + the section title in Chinese (no chapter number: the layout adds it).
- Sub-headings in the source (short title lines that are not sentences) become "## " lines.
- One paragraph per source paragraph, blank line between paragraphs. Keep "> " block quotes as "> ".
- Keep *emphasis*. Keep bullet lists as "- " lines.
- Proper nouns: company and product names stay in English (Google, Netflix, Y Combinator); people are
  given as 中文译名（English Name）the first time, then the Chinese name.
- Book and publication titles: 《中文译名》(English Title) the first time.
- Numbers, money and units stay as in the source (500 平方英尺, 2010 年秋天). Put one space between
  Chinese and Latin letters or digits.
- Some text carries image placeholders of the form ⟦IMG:key⟧ standing for a figure or equation stored as
  an image. Copy every ⟦IMG:...⟧ token EXACTLY as written, in the same place relative to the surrounding
  words. Never translate, renumber, merge, or drop one, and never invent new ones.
- Follow the glossary exactly when one is given.
`;

export const RULES_EPUB = `You are a professional literary translator (English -> Simplified Chinese) working on a published
non-fiction book. Everything you need is in the message: do not run commands, do not read or write files.
Translate the whole section, faithfully and fluently, as a Chinese publisher would print it. Never summarize,
never skip a paragraph, never add commentary.

The source is a fragment of the book's ORIGINAL EPUB XHTML, so its formatting tags are intact and carry meaning
you must preserve. Use them; do not print the tags themselves.

Math (the important part) -- convert inline math to correct LaTeX inside \\( ... \\), using the tags:
- <em><strong>x</strong></em> (bold italic) = a vector or matrix -> \\(\\mathbf{x}\\) (uppercase too, e.g. matrix \\(\\mathbf{A}\\)).
- a sans-serif capital (a tensor) -> \\(\\mathsf{A}\\).
- <em>x</em> (plain italic) = a scalar -> \\(x\\).
- <sub>i</sub> = subscript -> _{i};  <sup>2</sup> = superscript -> ^{2}. Keep superscript vs subscript straight,
  do not flip them (e.g. the L2 norm is \\(L^{2}\\) with a superscript; the norm selector is \\(\\|\\mathbf{x}\\|_2\\)).
- a transpose mark (superscript T or the character ⊤) -> ^{\\top}; never drop it.
- ℝ (often <span class="font3">ℝ</span>) -> \\(\\mathbb{R}\\); × -> \\times; ⊙ -> \\odot; ∈ -> \\in.

LaTeX that must parse (a renderer typesets it; malformed LaTeX prints as red error source):
- Close inline math with a half-width \\) -- never \\） or \\。 or any other character.
- A numbered equation is DISPLAY math: \\[ ... \\tag{9.3} \\]. Never put \\tag in inline \\( ... \\) (it is a parse error).
- Never wrap a display equation in a Markdown blockquote ("> "). Put \\[ ... \\] on its own, unindented.
- No Markdown inside math: write **bold** and function names as text OUTSIDE the \\( \\), not within it.
- A currency dollar sign inside math is \\$ (a bare $ is read as a math delimiter): \\(\\$100\\), or just write it as text.
- An image placeholder ⟦IMG:...⟧ is NEVER valid inside \\( \\) or \\[ \\]. If a symbol (e.g. an accented q̂) is only
  available as an image, write it as LaTeX instead (\\(\\hat{q}\\)); keep ⟦IMG⟧ tokens for real figures, outside math.

Emphasis and structure:
- <strong> around a word or term, and <em> used for prose emphasis (neither being a math variable) -> Chinese
  **bold**. (Chinese emphasises with bold, not italic; italicised Chinese is illegible. Math variables still
  become \\( \\) LaTeX, never bold.)
- Ignore navigation links (<a href="toc...">); keep only their visible text.
- Some fragments carry image placeholders ⟦IMG:key⟧ standing for a figure or displayed equation stored as an
  image. Copy every ⟦IMG:...⟧ token EXACTLY, in place; never translate, renumber, merge, drop, or invent one.

Highlights (a high bar -- be sparing):
- While translating, mark the section's MOST important content in **bold** as reading highlights: core
  conclusions, key definitions, the section's thesis. Usually 1-4 per section, each at most one sentence (a key
  phrase is better). NEVER bold whole paragraphs or several sentences in a row -- that ruins the page and defeats
  the purpose. When in doubt, mark less. Highlight bold uses the same **...** as term bold.

Output Markdown only:
- First line: "# " + the section title in Chinese (no chapter number: the layout adds it).
- Sub-headings become "## " lines. One paragraph per source paragraph, blank line between paragraphs.
- Proper nouns: company and product names stay in English; people are 中文译名（English Name）the first time.
- Book and publication titles: 《中文译名》(English Title) the first time.
- Numbers, money and units stay as in the source. Put one space between Chinese and Latin letters or digits.
- Follow the glossary exactly when one is given.
`;

export function loadSections(work) {
  const meta = JSON.parse(fs.readFileSync(path.join(work, "sections.json"), "utf8"));
  return [meta, meta.sections.filter((s) => s.file)];
}

export function buildPrompt(meta, section, text, glossary = null) {
  const label = section.label ? `${section.label} ` : "";
  const head = [`Book: ${meta.title}` + (meta.author ? ` by ${meta.author}` : ""),
    `Section: ${label}${section.title} (${section.kind}, ${Object.hasOwn(section, "words") ? section.words : "?"} words)`];
  if (glossary) head.push("Glossary (use these renderings):", pyStrip(glossary));
  return head.join("\n") + "\n\nSource text:\n\n" + pyStrip(text) + "\n";
}

export function cjkCount(s) {
  return (s.match(/[一-鿿]/g) || []).length;
}

/** A translation must start with the title heading and be long enough to have covered the source. The length
 * floor is 0.6 Chinese characters per English word: Chinese is more compact, and sections dense with names,
 * citations or math (a notation table, a bibliography, a history section) legitimately fall well under parity —
 * 0.9 kept false-rejecting complete translations. A real truncation lands far below 0.6, so it is still caught. */
export function checkOutput(md, words) {
  if (!pyStrip(md).startsWith("# ")) return "does not start with '# title'";
  if (words && cjkCount(md) < 0.6 * words) return `too short: ${cjkCount(md)} Chinese characters for ${words} English words`;
  return null;
}

export const OFF = ["shell_tool", "unified_exec", "unified_exec_tty", "view_image", "sleep_tool", "tool_suggest", "multi_agent",
  "plugins", "apps", "skill_search", "memories", "goals", "image_generation", "browser_use", "computer_use", "hooks"];

/** Python's "[Errno 2] No such file or directory: 'path'" for a missing file, so failures read the same. */
function pyOsError(e, file) {
  if (e && e.code === "ENOENT") return new Error(`[Errno 2] No such file or directory: '${file}'`);
  return e;
}

/** A private CODEX_HOME: the login copied from ~/.codex, no user config, AGENTS.md, plugins or hooks. */
export function codexHome(dir, model, effort, instructions, serviceTier) {
  const home = path.join(dir, "home");
  fs.mkdirSync(home, { recursive: true });
  const src = path.join(process.env.CODEX_HOME || path.join(os.homedir(), ".codex"), "auth.json");
  try {
    fs.copyFileSync(src, path.join(home, "auth.json"));
  } catch (e) {
    throw pyOsError(e, src);
  }
  const tier = serviceTier === "standard" || serviceTier === "default" ? "" : `service_tier = "${serviceTier}"\n`;
  fs.writeFileSync(path.join(home, "config.toml"), `model = "${model}"\nmodel_reasoning_effort = "${effort}"\n` +
    `model_instructions_file = "${instructions}"\nproject_doc_max_bytes = 0\n${tier}`);
  return home;
}

/** The codex exec command line for one translation call (stdin carries the prompt). */
export function codexArgs(dir, last) {
  const args = ["exec", "--ignore-rules", "--skip-git-repo-check", "--ephemeral", "-C", dir, "-s", "read-only"];
  for (const f of OFF) args.push("--disable", f);
  args.push("--json", "-o", last, "-");
  return args;
}

/** subprocess.run(args, input=, capture_output=True, text=True, timeout=): resolves {returncode, stdout, stderr},
 * or rejects with a TimeoutExpired-like error (code "ETIMEDOUT") after killing the child. */
function runProcess(cmd, args, input, env, timeoutMs) {
  return new Promise((resolve, reject) => {
    const child = spawn(cmd, args, { env, stdio: ["pipe", "pipe", "pipe"] });
    const out = [];
    const err = [];
    let timedOut = false;
    const timer = setTimeout(() => { timedOut = true; child.kill("SIGKILL"); }, timeoutMs);
    child.stdout.on("data", (d) => out.push(d));
    child.stderr.on("data", (d) => err.push(d));
    child.on("error", (e) => { clearTimeout(timer); reject(pyOsError(e, cmd)); });
    child.on("close", (code, signal) => {
      clearTimeout(timer);
      if (timedOut) { const e = new Error("timeout"); e.code = "ETIMEDOUT"; reject(e); return; }
      resolve({ returncode: code === null ? -os.constants.signals[signal] : code,
        stdout: Buffer.concat(out).toString("utf8"), stderr: Buffer.concat(err).toString("utf8") });
    });
    child.stdin.on("error", () => {});
    child.stdin.end(input);
  });
}

export async function runCodex(prompt, model, effort, serviceTier, { events = null, timeout = 3600, tries = 2, rules = RULES } = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "translate-"));
  const instructions = path.join(dir, "instructions.md");
  fs.writeFileSync(instructions, rules);
  const last = path.join(dir, "last.txt");
  const home = codexHome(dir, model, effort, instructions, serviceTier);
  const args = codexArgs(dir, last);
  const t0 = Date.now();
  let err = null;
  let r = null;
  let ok = false;
  for (let i = 0; i < tries; i++) {
    try {
      r = await runProcess("codex", args, prompt, { ...process.env, CODEX_HOME: home }, timeout * 1000);
    } catch (e) {
      if (e.code !== "ETIMEDOUT") throw e;
      err = `codex exec timed out after ${timeout}s`;
      continue;
    }
    if (events) fs.writeFileSync(events, r.stdout);
    if (r.returncode === 0 && fs.existsSync(last)) { ok = true; break; }
    err = `codex exec exited ${r.returncode}: ${r.stderr.slice(-2000)}`;
  }
  if (!ok) throw new Error(err);
  let usage = null;
  for (const line of r.stdout.split(/\r?\n/)) {
    let ev;
    try {
      ev = JSON.parse(line);
    } catch {
      continue;
    }
    if (ev && ev.type === "turn.completed") usage = ev.usage ?? null;
  }
  return [fs.readFileSync(last, "utf8"), usage, Math.round((Date.now() - t0) / 1000)];
}

function stemOf(file) {
  const name = path.basename(file);
  const i = name.lastIndexOf(".");
  return i > 0 ? name.slice(0, i) : name;
}

export async function translateOne(work, meta, section, opt) {
  const text = fs.readFileSync(path.join(work, section.file), "utf8");
  const prompt = buildPrompt(meta, section, text, opt.glossaryText);
  const rules = meta.source_kind === "epub" ? RULES_EPUB : RULES;
  const out = path.join(work, "translated", `${stemOf(section.file)}.md`);
  const events = path.join(work, "translated", `${section.id}.events.jsonl`);
  let problem = null;
  let md, usage, seconds;
  for (const attempt of [1, 2]) {
    [md, usage, seconds] = await runCodex(prompt, opt.model, opt.effort, opt.serviceTier, { events, rules });
    problem = checkOutput(md, section.words);
    if (!problem) break;
  }
  if (problem) {
    fs.writeFileSync(path.join(work, "translated", `${section.id}.rejected.md`), md);
    throw new Error(`${section.id} ${problem} (kept as ${section.id}.rejected.md)`);
  }
  fs.writeFileSync(out, pyStrip(md) + "\n");
  return `${section.id} ${section.title}: ${cjkCount(md)} chars, ${seconds}s, usage ${pyDumps(usage)}`;
}

const USAGE = "usage: translate.mjs [-h] [--only ONLY] [--jobs JOBS] [--model MODEL] [--effort EFFORT] [--service-tier SERVICE_TIER]\n" +
  "                     [--glossary GLOSSARY] [--force] [--dry-run]\n                     work\n";

function usageError(message) {
  process.stderr.write(USAGE + `translate.mjs: error: ${message}\n`);
  process.exit(2);
}

/** argparse-compatible: positional work and the options above (also --flag=value). */
export function parseArgs(argv) {
  const opt = { work: null, only: null, jobs: 20, model: "gpt-6-luna", effort: "low", serviceTier: "priority",
    glossary: null, force: false, dryRun: false };
  const valued = { "--only": "only", "--jobs": "jobs", "--model": "model", "--effort": "effort", "--service-tier": "serviceTier", "--glossary": "glossary" };
  const flags = { "--force": "force", "--dry-run": "dryRun" };
  const positional = [];
  for (let i = 0; i < argv.length; i++) {
    let a = argv[i];
    if (a === "-h" || a === "--help") {
      process.stdout.write(USAGE + "\npositional arguments:\n  work\n\noptions:\n  -h, --help            show this help message and exit\n" +
        "  --only ONLY           comma-separated section ids\n  --jobs JOBS\n  --model MODEL\n  --effort EFFORT\n" +
        "  --service-tier SERVICE_TIER\n  --glossary GLOSSARY\n  --force               retranslate sections that already have an .md\n" +
        "  --dry-run             print the first section's prompt and exit\n");
      process.exit(0);
    }
    if (a === "--") { positional.push(...argv.slice(i + 1)); break; }
    if (a.startsWith("--")) {
      let value = null;
      if (a.includes("=")) { value = a.slice(a.indexOf("=") + 1); a = a.slice(0, a.indexOf("=")); }
      if (Object.hasOwn(flags, a)) {
        if (value !== null) usageError(`argument ${a}: ignored explicit argument '${value}'`);
        opt[flags[a]] = true;
        continue;
      }
      if (Object.hasOwn(valued, a)) {
        if (value === null) { if (i + 1 >= argv.length) usageError(`argument ${a}: expected one argument`); value = argv[++i]; }
        if (a === "--jobs") {
          if (!/^\s*[+-]?\d+\s*$/.test(value)) usageError(`argument --jobs: invalid int value: '${value}'`);
          value = parseInt(value, 10);
        }
        opt[valued[a]] = value;
        continue;
      }
      usageError(`unrecognized arguments: ${a}`);
    }
    positional.push(a);
  }
  if (!positional.length) usageError("the following arguments are required: work");
  if (positional.length > 1) usageError(`unrecognized arguments: ${positional.slice(1).join(" ")}`);
  opt.work = positional[0];
  return opt;
}

/** Runs tasks with at most `jobs` in flight, calling onDone(result, error, task) as each one completes. */
async function runPool(tasks, jobs, start, onDone) {
  let next = 0;
  const worker = async () => {
    while (next < tasks.length) {
      const task = tasks[next++];
      try {
        onDone(await start(task), null, task);
      } catch (e) {
        onDone(null, e, task);
      }
    }
  };
  await Promise.all(Array.from({ length: Math.max(1, Math.min(jobs, tasks.length)) }, worker));
}

export async function main(argv) {
  const opt = parseArgs(argv);
  const work = path.resolve(opt.work);
  const [meta, allSections] = loadSections(work);
  let sections = allSections;
  fs.mkdirSync(path.join(work, "translated"), { recursive: true });
  opt.glossaryText = opt.glossary ? fs.readFileSync(opt.glossary, "utf8") : null;
  if (opt.only) {
    const wanted = new Set(opt.only.split(","));
    sections = sections.filter((s) => wanted.has(s.id));
  }
  const mdOf = (s) => path.join(work, "translated", `${stemOf(s.file)}.md`);
  const todo = sections.filter((s) => opt.force || !fs.existsSync(mdOf(s)));
  if (opt.dryRun) {
    const s = todo.length ? todo[0] : sections[0];
    const rules = meta.source_kind === "epub" ? RULES_EPUB : RULES;
    process.stdout.write(rules + "\n-----\n" + buildPrompt(meta, s, fs.readFileSync(path.join(work, s.file), "utf8"), opt.glossaryText) + "\n");
    return;
  }
  process.stdout.write(`${todo.length} of ${sections.length} sections to translate with ${opt.model}/${opt.effort}/${opt.serviceTier}, ` +
    `${opt.jobs} in flight\n`);
  let failed = 0;
  await runPool(todo, opt.jobs, (s) => translateOne(work, meta, s, opt), (result, error, s) => {
    if (error) {
      failed += 1;
      process.stdout.write(`FAILED ${s.id}: ${error.message}\n`);
    } else {
      process.stdout.write(`${result}\n`);
    }
  });
  const done = sections.filter((s) => fs.existsSync(mdOf(s))).length;
  process.stdout.write(`done: ${done}/${sections.length} sections translated, ${failed} failed\n`);
  process.exit(failed ? 1 : 0);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === fs.realpathSync(process.argv[1])) {
  main(process.argv.slice(2)).catch((e) => { console.error(e); process.exit(1); });
}
