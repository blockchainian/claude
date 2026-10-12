#!/usr/bin/env node
// ABOUTME: Translates a paper work dir (from pdf2work.py) section by section through the translate-epub skill's codex
// ABOUTME: runner, with paper rules and a math check on top of the skill's output checks; resumable.
//
// Usage: translate_paper.mjs <work dir> [--only 02,03] [--jobs 20] [--model gpt-6-luna] [--effort medium]
//                            [--glossary <file>] [--glossaries <dir>] [--force]
// Without --glossary it uses <work>/glossary.md, writing it first with one codex call when it is missing, seeded
// with the course glossary <dir>/<course>.md (default <INTEL_STATE_DIR>/translate-glossaries).
import { createHash } from "node:crypto";
import fs from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { stateDir } from "../../../fetch-x-mentions/scripts/env.mjs";
import { buildPrompt, checkOutput, cjkCount, runCodex } from "../../epub/scripts/translate.mjs";

const katex = createRequire(import.meta.url)("katex");

export const RULES_PAPER = `You are a professional translator (English -> Simplified Chinese) of computer-science papers and
lecture notes. Everything you need is in the message: do not run commands, do not read or write files.
Translate the whole section, faithfully and fluently, as a Chinese academic publisher would print it. Never
summarize, never skip a paragraph, never add commentary.

The source is Markdown that a layout model extracted from the PDF. After it comes the PDF's own text layer for the
same passage: use it only to check words and symbols the layout model may have misread, never translate it
separately.

Math:
- Inline math is \\( ... \\) and display math is \\[ ... \\] on its own line. Copy every formula exactly, including
  \\tag{...}. Never translate, reorder, merge or drop a formula, and never move prose into it. A single variable
  such as \\(k\\) or \\(k + 1\\) stays a formula too, even where the source sets the same letter as plain text
  elsewhere; never turn a formula into plain text except the misreads below.
- The formula recognizer sometimes misreads plain text as math. Fix only such clear misreads, checking the text
  layer: an English word set as math (\\(\\mathbf{\\ddot{big}}\\) for the word "big") becomes the plain word, and a
  stray accent or bar on a single letter that the text layer shows as a plain letter is removed. Leave every
  other formula untouched.
- Put sentence punctuation outside the formula.
- Never put \\( \\) inside \\text{} or nest one formula in another. Keep Chinese words outside the formula and
  split it around them: \\(P_{jk}\\) =（\\(V\\) 的第 \\(j\\) 行）, not \\(P_{jk} = (\\text{\\(V\\) 的第 }j\\text{ 行})\\).

Structure (keep it exactly):
- First line: "# " + the section title in Chinese, keeping its number ("# 1. INTRODUCTION" -> "# 1. 引言").
- Keep "## " and "### " headings at their level, translated. One paragraph per source paragraph, blank line
  between paragraphs. Keep list items as list items with their markers ("- ", "1.", "(a)") and indentation.
- Keep pipe tables as pipe tables with the same rows and columns; translate the prose cells.
- A figure, table or algorithm caption starts with a bold label: "**Fig. 3.** ..." -> "**图 3.** ...",
  "**Figure 3:**" -> "**图 3：**", "**Table 2.**" -> "**表 2.**", "**Algorithm 1**" -> "**算法 1**".
- Copy every ⟦IMG:...⟧ and ⟦CODE:...⟧ token exactly, alone on its line, in its place. Never invent one.
- Superscript digits written as characters (L⁴Linux, n²) stay characters; <sup> is only for footnote marks.
- Keep every <sup>N</sup> footnote mark, and the "---" line that separates the section's footnotes from its body;
  translate the footnotes below it.
- **bold** stays bold. Do not add bold of your own.
- Keep every Markdown link [text](url) as a link with the same URL; translate its text only when it is prose.

Wording:
- Technical terms take their standard Chinese rendering; on first use in the section, add the English in
  parentheses: 缓存无关（cache-oblivious）. Algorithm and procedure names in capitals (REC-MULT, TILED-MULT),
  program identifiers, file names, URLs, DOIs and e-mail addresses stay as they are.
- Citations stay as in the source: [Aggarwal et al. 1987a], [28, 33]. Author names in bylines and citations stay in
  English.
- Put one space between Chinese and Latin letters or digits. Use Chinese punctuation in Chinese sentences.
- Follow the glossary exactly when one is given.
`;

const MATH_RE = /\\\[[\s\S]*?\\\]|\\\([\s\S]*?\\\)/g;

/** True when KaTeX typesets a \\(..\\) or \\[..\\] formula cleanly, with the renderer's settings: render.py's
 * auto-render (throwOnError off, default strictness) prints a parse error as red source and an unsupported
 * character (a Unicode ² inside math) in red, and fails the render on either. */
function parses(m) {
  const tex = m.slice(2, -2).replace(/(?<!\\)\$/g, "\\$");  // render.py's repair_math reads every bare $ as currency
  const html = katex.renderToString(tex, { displayMode: m.startsWith("\\["), throwOnError: false });
  return !/katex-error|#cc0000/i.test(html);
}

/** The paper-specific check on top of the skill's: every display equation and nearly every inline formula came
 * back (a formula may move from inline to display). Formulas are not compared verbatim: the translator tidies
 * MinerU's spacing and repairs its misreads against the text layer (a stray \bar, a year read as \mathbf{1987a}),
 * which a verbatim check would reject. */
/** True for a "formula" that is really a word MinerU set as math (\\(\\mathrm { C } { + } { + }\\) for C++,
 * \\(\\mathbf { \\ddot { b i g } }\\) for "big"): the translator is told to write it as text, so it does not count. */
export function wordAsMath(m) {
  if (plainText(m)) return true;  // a name with a script (TRANSFER₁, L⁴, R*) reads the same written as text
  const core = m.slice(2, -2).replace(/\\(mathrm|mathbf|mathit|mathsf|mathtt|operatorname|text|textbf|texttt|tilde|ddot|dot|bar|hat)\b/g, "")
    .replace(/\\mu\b/g, "µ").replace(/[{}\s^_]/g, "");
  // a lone letter or Greek letter (µ in "µ-kernel"): written as text it reads the same, so it does not count as a
  // lost formula
  if (/^([A-Za-zµ]|\\(alpha|beta|gamma|delta|epsilon|lambda|pi|sigma|tau|phi|omega|theta))$/.test(core)) return true;
  if (/\\mathrm/.test(m) && /^[A-Z]\d$/.test(core)) return true;  // an upright name with a raised digit: L⁴
  // a measurement or percentile set as math: 475 µs, 95th, p75
  if (/^[pP]?\d[\d.,]*[A-Za-zµ%]{0,4}$/.test(core)) return true;
  // a word, or a lone relation sign (\\(<\\)) that reads as text either way
  return /^[A-Za-z][A-Za-z+'’./-]+$/.test(core) || /^[<>=+−-]$/.test(core);
}

const SUP = { 0: "⁰", 1: "¹", 2: "²", 3: "³", 4: "⁴", 5: "⁵", 6: "⁶", 7: "⁷", 8: "⁸", 9: "⁹" };
const SUB = { 0: "₀", 1: "₁", 2: "₂", 3: "₃", 4: "₄", 5: "₅", 6: "₆", 7: "₇", 8: "₈", 9: "₉" };

const DIGIT = Object.fromEntries([...Object.entries(SUP), ...Object.entries(SUB)].map(([d, c]) => [c, d]));

/** The ways a translator writes a short formula as text: L⁴, L4 and L<sup>4</sup> for \\(L^{4}\\); none for real math. */
export function plainForms(m) {
  const t = plainText(m);
  if (!t) return [];
  return [t, t.replace(/[⁰-⁹₀-₉¹²³]/g, c => DIGIT[c]),
    t.replace(/[⁰¹²³⁴⁵⁶⁷⁸⁹]+/g, d => `<sup>${[...d].map(c => DIGIT[c]).join("")}</sup>`),
    t.replace(/[₀-₉]+/g, d => `<sub>${[...d].map(c => DIGIT[c]).join("")}</sub>`),
    t.replace(/[₀-₉]+/g, d => `_${[...d].map(c => DIGIT[c]).join("")}`)];
}

/** A short formula's plain-text form with Unicode super- and subscript digits (L^{4} -> L⁴, x_{1} -> x₁, R^{*} -> R*), or null
 * when it holds anything beyond letters, digits and those scripts. */
export function plainText(m) {
  let t = m.slice(2, -2).replace(/\\(mathrm|mathit|mathsf|mathtt|mathbb|text)\b/g, "").replace(/[\s{}]/g, "")
    .replace(/\^\*/g, "*");
  t = t.replace(/\^(\d+)/g, (_, d) => [...d].map(c => SUP[c]).join(""))
    .replace(/_(\d+)/g, (_, d) => [...d].map(c => SUB[c]).join(""));
  return /^[A-Za-z]+([⁰-⁹₀-₉¹²³]+|\*)$/.test(t) ? t : null;  // L⁴, x₁, or a starred name such as R*
}

export function checkMath(md, source) {
  const src = (source.match(MATH_RE) || []).filter(m => !wordAsMath(m));
  const out = md.match(MATH_RE) || [];
  const display = s => s.filter(m => m.startsWith("\\[")).length;
  // more display equations is fine: the translator may set a formula MinerU read inline as display, as the PDF does
  if (display(out) < display(src)) return `display math: ${display(out)} of ${display(src)}`;
  // a short formula the translator wrote as text with Unicode super/subscripts (\\(L^{4}\\) -> L⁴) is not lost
  const prose = md.replace(MATH_RE, " ");
  const asText = src.filter(m => plainForms(m).some(v => prose.includes(v))).length;
  if (out.length + asText < 0.9 * src.length - 1) return `inline math: ${out.length} of ${src.length} formulas`;
  // A formula KaTeX cannot parse (the translator nests "\\(m, \\(p" or unbalances a brace) prints as red source.
  // "$...$" is not a delimiter here: the renderer prints it as raw source
  const dollars = text => (text.replace(MATH_RE, "").match(/(?<!\\)\$[^$\n]+(?<!\\)\$/g) || []).length;
  if (dollars(md) > dollars(source)) return `math in $...$ instead of \\(...\\)`;
  const broken = list => list.filter(m => !parses(m));
  const bad = broken(out), badSrc = broken(src);
  if (bad.length > badSrc.length) return `${bad.length} formulas KaTeX cannot parse, e.g. ${bad[0].slice(0, 80)}`;
  return null;
}

/** Every link target of the source survives in the translation. */
export function checkLinks(md, source) {
  const urls = text => [...text.matchAll(/\]\((https?:\/\/[^)\s]+)\)/g)].map(m => m[1]);
  const missing = urls(source).filter(u => !urls(md).includes(u));
  return missing.length ? `links lost: ${missing.join(", ")}` : null;
}

export const RULES_GLOSSARY = `You prepare the glossary for translating one computer-science paper or set of lecture notes from
English into Simplified Chinese. Everything you need is in the message: do not run commands, do not read or write
files. Reply with the glossary in Markdown only, in this form:

Title: <English title> = <Chinese title>

Section titles:
- <English heading> = <Chinese heading>    (every heading given, numbers kept: "1. INTRODUCTION = 1. 引言")

Terms:
- <English term> = <Chinese term>    (20 to 40 key technical terms of this text, the standard Chinese renderings
  used in Chinese computer-science textbooks; names of algorithms, programs and systems that stay in English are
  listed as "keep as is")`;

/** The course's shared glossary file ("english: 中文" per line): <dir>/<key>.md, the key being the course number
 * that opens the source PDF's folder name ("6.1220-Design and Analysis of Algorithms" -> 6.1220), else the
 * folder name itself. */
export function courseGlossaryPath(meta, dir) {
  if (!meta.source_pdf) return null;
  const folder = path.basename(path.dirname(meta.source_pdf));
  const key = (folder.match(/^\d+(\.\d+)*/) || [folder])[0];
  return path.join(dir, `${key}.md`);
}

/** Appends the paper glossary's terms that the course glossary lacks, in one write. */
export function appendCourseTerms(coursePath, paperGlossary) {
  const have = fs.existsSync(coursePath) ? fs.readFileSync(coursePath, "utf8") : "";
  const known = new Set(have.split("\n").map(l => l.split(":")[0].trim().toLowerCase()).filter(Boolean));
  const terms = paperGlossary.split(/^Terms:\s*$/m)[1] || "";
  const fresh = [];
  for (const line of terms.split("\n")) {
    const m = line.match(/^-\s*(.+?)\s*=\s*(.+?)\s*$/);
    if (!m || /keep as is/i.test(m[2]) || known.has(m[1].toLowerCase())) continue;
    known.add(m[1].toLowerCase());
    fresh.push(`${m[1]}: ${m[2]}`);
  }
  if (fresh.length) {
    fs.mkdirSync(path.dirname(coursePath), { recursive: true });
    fs.appendFileSync(coursePath, (have && !have.endsWith("\n") ? "\n" : "") + fresh.join("\n") + "\n");
  }
  return fresh.length;
}

/** One codex call that writes <work>/glossary.md from the section headings and the opening of the text, seeded
 * with the course glossary so a course's notes and papers render terms alike; its new terms go back to it. */
async function makeGlossary(work, meta, opt) {
  const sources = meta.sections.filter(s => s.file && !s.verbatim).map(s => fs.readFileSync(path.join(work, s.file), "utf8"));
  const headings = sources.flatMap(t => t.split("\n").filter(l => /^#{1,3} /.test(l)).map(l => l.replace(/^#+ /, "")));
  const sample = sources.join("\n\n").split(/\s+/).slice(0, 4000).join(" ");
  const coursePath = courseGlossaryPath(meta, opt.glossaries);
  const course = coursePath && fs.existsSync(coursePath) ? fs.readFileSync(coursePath, "utf8") : "";
  const seed = course ? `Course glossary (reuse these renderings exactly; list a term from it only if this text uses it):\n${course}\n` : "";
  const prompt = seed + `Title: ${meta.title}\n\nHeadings:\n${headings.map(h => "- " + h).join("\n")}\n\nOpening of the text:\n\n${sample}\n`;
  const [md] = await runCodex(prompt, opt.model, "low", opt.serviceTier, { rules: RULES_GLOSSARY });
  fs.writeFileSync(path.join(work, "glossary.md"), md.trim() + "\n");
  if (coursePath) console.log(`course glossary ${path.basename(coursePath)}: ${appendCourseTerms(coursePath, md)} new terms`);
  return md;
}

export const RULES_BYLINE = `You read the byline of a paper (author names and affiliations, as extracted from its first page) and
return the authors as JSON only, no prose and no code fence:
{"authors": [{"name": "Matteo Frigo", "affiliation": "麻省理工学院计算机科学实验室"}, ...], "shared": "麻省理工学院计算机科学实验室"}
- One entry per author, in byline order. Names in normal capitalisation ("MATTEO FRIGO" -> "Matteo Frigo"),
  without footnote marks (*, †, ‡, digits).
- affiliation: the author's institution (lab, school or company, with its place when printed) in Simplified
  Chinese, using its standard Chinese rendering ("Department of Computer Science, Princeton University" ->
  "普林斯顿大学计算机科学系"), with a printed place in Chinese order after it in full-width brackets
  ("Intel Corporation, Nashua, New Hampshire" -> "英特尔公司（新罕布什尔州纳舒厄）"); a proper name with no
  standard Chinese form stays in English. null when the byline gives none.
- shared: the affiliation, in the same Chinese form, when every author has the same one, else null.
- Names stay in English.
Everything you need is in the message: do not run commands, do not read or write files.`;

/** The cover's author lines: each author with their affiliation, read from the byline by one codex call. */
async function coverAuthors(meta, opt) {
  const [answer] = await runCodex("Byline:\n\n" + meta.byline.join("\n") + "\n", opt.model, "low", opt.serviceTier, { rules: RULES_BYLINE });
  const parsed = JSON.parse(answer.trim().replace(/^```(?:json)?\s*|\s*```$/g, ""));
  if (!Array.isArray(parsed.authors) || !parsed.authors.every(a => typeof a.name === "string")) throw new Error(`bad byline answer: ${answer}`);
  return parsed;
}

/** A <sup>N</sup> the translator made of a superscript the source writes as characters (L⁴ -> L<sup>4</sup>)
 * goes back to characters, when N is not one of the source's footnote marks, so it is not taken for a footnote. */
export function unmarkExponents(md, source) {
  const marks = new Set([...source.matchAll(/<sup>\s*(\d+)\s*<\/sup>/g)].map(m => m[1]));
  return md.replace(/<sup>(\d+)<\/sup>/g, (all, d) => marks.has(d) ? all : [...d].map(c => SUP[c]).join(""));
}

/** Puts back figure and code placeholders the translator dropped. The tokens still present split source and
 * translation into matching stretches; within a stretch whose translation has exactly as many fewer paragraphs
 * as the stretch lost tokens, every other paragraph lines up one to one (one paragraph per source paragraph is a
 * rule), so each lost token goes back to its source position; within a stretch of equal length, the paragraph
 * the translator wrote where a token stood is replaced by it. Headings anchor stretches too, by position. A stretch
 * that does not line up is left as is. */
export function restoreTokens(md, source) {
  const blocks = text => text.trim().split(/\n\s*\n/);
  const isToken = b => /^⟦(?:IMG|CODE):[^⟧]+⟧$/.test(b.trim());
  const src = blocks(source), out = blocks(md);
  const kept = new Set(out.filter(isToken).map(b => b.trim()));
  if (!src.some(b => isToken(b) && !kept.has(b.trim()))) return md;
  // anchors: the tokens still present, and headings (translated, so matched by position, not text)
  const isHeading = b => /^#{1,6} /.test(b.trim());
  const stretches = list => {  // [[blocks...], {anchor}, [blocks...], {anchor}, ...]
    const parts = [[]];
    for (const b of list) {
      if (isToken(b) && kept.has(b.trim())) parts.push({ key: b.trim(), block: b.trim() }, []);
      else if (isHeading(b)) parts.push({ key: "#", block: b }, []);
      else parts[parts.length - 1].push(b);
    }
    return parts;
  };
  const s = stretches(src), o = stretches(out);
  const keys = parts => parts.filter(p => !Array.isArray(p)).map(p => p.key).join("\n");
  if (s.length !== o.length || keys(s) !== keys(o)) return md;
  const merged = [];
  for (let i = 0; i < s.length; i++) {
    if (!Array.isArray(s[i])) { merged.push(o[i].block); continue; }
    const lost = s[i].filter(isToken);
    if (lost.length && o[i].length === s[i].length - lost.length) {
      let j = 0;
      for (const b of s[i]) merged.push(isToken(b) ? b.trim() : o[i][j++]);
    } else if (lost.length && o[i].length === s[i].length) {
      // same count: the translator wrote something of its own where the token stood ("（代码）"); the token goes back
      merged.push(...s[i].map((b, k) => isToken(b) ? b.trim() : o[i][k]));
    } else merged.push(...o[i]);
  }
  return merged.join("\n\n") + "\n";
}

async function translateSection(work, meta, s, opt) {
  const text = fs.readFileSync(path.join(work, s.file), "utf8");
  const refPath = path.join(work, s.file.replace(/\.md$/, ".ref.txt"));
  const ref = fs.existsSync(refPath) ? fs.readFileSync(refPath, "utf8") : "";
  const prompt = buildPrompt(meta, s, text, opt.glossaryText) +
    (ref ? "\nPDF text layer of the same passage (for checking only):\n\n" + ref + "\n" : "");
  const out = path.join(work, "translated", `${path.basename(s.file, ".md")}.md`);
  const events = path.join(work, "translated", `${s.id}.events.jsonl`);
  let md, usage, seconds, problem;
  for (let attempt = 0; attempt < 2; attempt++) {
    [md, usage, seconds] = await runCodex(prompt, opt.model, opt.effort, opt.serviceTier, { events, rules: RULES_PAPER });
    md = restoreTokens(md, text);
    md = unmarkExponents(md, text);
    md = md.replace(/\[\[([^\]]*)\]\((https?:[^)\s]+)\)\]\(\2\)/g, "[$1]($2)");  // a link the translator nested in itself
    problem = checkOutput(md, s.words, text) || checkMath(md, text) || checkLinks(md, text);
    if (!problem) break;
  }
  if (problem) {
    fs.writeFileSync(path.join(work, "translated", `${s.id}.rejected.md`), md);
    throw new Error(`${problem} (kept as ${s.id}.rejected.md)`);
  }
  fs.writeFileSync(out, md.trim() + "\n");
  fs.writeFileSync(out.replace(/\.md$/, ".src-sha1"), sha1(text));
  return `${s.id} ${s.title}: ${cjkCount(md)} chars, ${seconds}s, ${usage ? usage.output_tokens : "?"} output tokens`;
}

const sha1 = text => createHash("sha1").update(text).digest("hex");

function parseArgs(argv) {
  const opt = { only: null, jobs: 20, model: "gpt-6-luna", effort: "medium", serviceTier: "priority", glossary: null,
    glossaries: process.env.TRANSLATE_GLOSSARIES || path.join(stateDir(), "translate-glossaries"), force: false };
  const pos = [];
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const key = { "--only": "only", "--jobs": "jobs", "--model": "model", "--effort": "effort", "--glossary": "glossary",
      "--glossaries": "glossaries", "--service-tier": "serviceTier" }[a];
    if (key) opt[key] = argv[++i];
    else if (a === "--force") opt.force = true;
    else pos.push(a);
  }
  if (pos.length !== 1) {
    console.error("usage: translate_paper.mjs <work dir> [--only ids] [--jobs N] [--model M] [--effort E] [--glossary F] [--glossaries DIR] [--force]");
    process.exit(2);
  }
  opt.work = path.resolve(pos[0]);
  opt.jobs = Number(opt.jobs);
  return opt;
}

async function main() {
  const opt = parseArgs(process.argv.slice(2));
  const meta = JSON.parse(fs.readFileSync(path.join(opt.work, "sections.json"), "utf8"));
  const glossaryPath = opt.glossary || path.join(opt.work, "glossary.md");
  if (!fs.existsSync(glossaryPath)) {
    await makeGlossary(opt.work, meta, opt);
    console.log(`glossary: wrote ${glossaryPath}`);
  }
  opt.glossaryText = fs.readFileSync(glossaryPath, "utf8");
  fs.mkdirSync(path.join(opt.work, "translated"), { recursive: true });
  if (meta.byline?.length && (!meta.cover_authors || opt.force && !opt.only)) {
    meta.cover_authors = await coverAuthors(meta, opt);
    fs.writeFileSync(path.join(opt.work, "sections.json"), JSON.stringify(meta, null, 1));
    console.log(`cover: ${meta.cover_authors.authors.map(a => a.name).join(", ")}`);
  }
  const wanted = opt.only ? new Set(opt.only.split(",")) : null;
  // a section counts as translated unless its source changed since (pdf2work rerun after a converter fix); a
  // translation written before source hashes were kept counts as current
  const done = s => {
    const out = path.join(opt.work, "translated", `${path.basename(s.file, ".md")}.md`);
    if (!fs.existsSync(out)) return false;
    const hash = out.replace(/\.md$/, ".src-sha1");
    return s.verbatim || !fs.existsSync(hash) || fs.readFileSync(hash, "utf8") === sha1(fs.readFileSync(path.join(opt.work, s.file), "utf8"));
  };
  const todo = meta.sections.filter(s => s.file && !s.verbatim && (!wanted || wanted.has(s.id)) && (opt.force || !done(s)));
  console.log(`${todo.length} sections to translate with ${opt.model}/${opt.effort}, ${opt.jobs} in flight`);
  let failed = 0, next = 0;
  const worker = async () => {
    while (next < todo.length) {
      const s = todo[next++];
      try { console.log(await translateSection(opt.work, meta, s, opt)); }
      catch (e) { failed++; console.log(`FAILED ${s.id}: ${e.message}`); }
    }
  };
  await Promise.all(Array.from({ length: Math.max(1, Math.min(opt.jobs, todo.length)) }, worker));
  console.log(`done: ${meta.sections.filter(done).length}/${meta.sections.length} sections, ${failed} failed`);
  process.exit(failed ? 1 : 0);
}

if (process.argv[1] && fs.realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) main();
