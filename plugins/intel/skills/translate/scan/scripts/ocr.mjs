#!/usr/bin/env node
// ABOUTME: Transcribes each page image of a scanned or handwritten PDF into Markdown with one Responses call per
// ABOUTME: page (the image attached), many pages in flight; optionally a proofreading call per page. Resumable.
//
// Usage: ocr.mjs <work dir> [--mode typeset|handwritten|slide] [--proofread] [--jobs 12] [--model M] [--effort low]
// Reads <work>/pages/p-NN.png (any zero padding); writes <work>/ocr/NN.md, and with --proofread <work>/proof/NN.md: the draft with
// each spot where an independent macOS Vision OCR disagrees with it re-read blind from a crop (log in
// proof/NN.edits.txt).
import { prepareImage } from "./images.mjs";
import { imageInput, requestCodex } from "../../epub/scripts/codex.mjs";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { loadEnvFile } from "../../../fetch-x-mentions/scripts/env.mjs";
import { applyReadings, cropSpots, disagreements, proofPrompt, READINGS_SCHEMA } from "./proof.mjs";
import { visionLines } from "./vision.mjs";

const COMMON = `- Math in LaTeX: inline \\( ... \\), display \\[ ... \\] on its own lines. No Markdown inside math.
- A figure, diagram, plot, drawing or sketch becomes one line [FIGURE x0,y0,x1,y1]: its bounding box as fractions
  (0..1, three decimals) of page width and height, tight around the drawing and the labels drawn in it, and
  excluding the transcribed text lines above, below and beside it. Never transcribe the text inside a figure.
- A figure's caption follows its [FIGURE] line as its own paragraph in bold: **Figure 1: ...**.
- A table of text becomes a Markdown table; a table that is mostly lines and drawing is a figure.
- Program code or pseudo-code becomes a fenced code block, keeping its line breaks and indentation.
- Output only the Markdown, with no commentary and no code fence around the whole answer.`;

export const PROMPTS = {
  typeset: `Transcribe this scanned page of a printed paper into Markdown, word for word.
- Reading order: for two columns, the whole left column, then the right column.
- One paragraph per line; rejoin words hyphenated across line breaks. Keep a word hyphenated at the very end of
  the page as printed, with its trailing hyphen.
- If the page's first text continues a paragraph from the previous page (it starts mid-sentence or mid-paragraph,
  with no indent), begin your output with a line holding only <<CONT>>.
- The paper's title "# Title"; authors and affiliations one plain paragraph; section headings "## Heading",
  sub-section headings "### Heading", keeping any printed number ("## 2 Primitives").
- Italic *x*, bold **x**. Small capitals as uppercase. Footnotes at the page bottom: a paragraph starting
  "Note: " placed at the end of the page.
- Drop running heads and page numbers.
- A monospaced code/source listing MUST be transcribed in a triple-backtick fenced code block, preserving
  its line breaks, indentation and literal characters. This includes TeX/LaTeX source, WEB output, macro
  definitions, incomplete source fragments and English sentences printed as part of the listing.
- In source code, \\input, \\], \\[5] and \\( are literal characters, not mathematical delimiters. Do not
  balance them, render them as math, or add Markdown escapes to them.
- A source listing printed as a figure is still code to transcribe, not a [FIGURE] crop. Keep its caption outside
  the code fence. If the page consists of source code, fence the entire listing even though the general rule
  below forbids a code fence around the whole answer; that rule applies to prose and captions, not source code.
${COMMON}`,
  slide: `Transcribe the text of this lecture slide into Markdown, word for word.
- The slide title as "## Title"; bullets as "- " items, indented two spaces per level; other text as one
  paragraph per line, rejoining a paragraph's wrapped lines; in reading order (top to bottom, left column before
  right). A short label or table cell standing on its own is its own line.
- Keep the source's symbols and digits exactly (a zero is 0, not O).
${COMMON}`,
  handwritten: `Transcribe this page of handwritten lecture notes into Markdown, faithfully, word for word.
- Keep the outline: an underlined heading line becomes "## Heading"; dash items become "- " bullets, indented by
  two spaces per level to show nesting; a line under a bullet that continues it stays in the same item.
- Arrows that start a line (↳, ⇒, →) stay as that character at the start of the item.
- Circled letters such as Ⓐ are written (A). Underlined words become **bold**. Ink colour does not matter.
- A header line with the course number, lecture number and date is one plain line: "6.046 · Lecture 3 · Feb. 12, 2015".
- "lg" is \\lg. Transcribe what is written; never correct the author. An illegible word is [?].
${COMMON}`,
};

/** One tool-free Responses call with images prepared at the CLI high-detail resolution. */
export async function codexWithImage(prompt, images, { model, effort, serviceTier = "priority", timeout = 900, schema = null, events = null }) {
  const paths = [images].flat();
  const bytes = await Promise.all(paths.map(prepareImage));
  const [answer] = await requestCodex(imageInput(prompt, paths, bytes), model, effort, serviceTier,
    { events, timeout, schema, rules: "You transcribe page images exactly. Do not run commands or touch files.\n" });
  return answer.trim() + "\n";
}

/** True when every \\( has its \\) and every \\[ its \\], in order: an unclosed formula turns the rest of the
 * page into raw LaTeX in the render. */
export function mathBalanced(md) {
  const prose = md.replace(/```[\s\S]*?(?:```|$)|`[^`]*`/g, "");
  let open = null;
  for (const m of prose.matchAll(/\\([()[\]])/g)) {
    const d = m[1];
    if (d === "(" || d === "[") {
      if (open) return false;
      open = d;
    } else {
      if (open !== (d === ")" ? "(" : "[")) return false;
      open = null;
    }
  }
  return open === null;
}

async function pool(items, jobs, fn) {
  let next = 0;
  const worker = async () => { while (next < items.length) await fn(items[next++]); };
  await Promise.all(Array.from({ length: Math.min(jobs, items.length) }, worker));
}

export const OCR_DEFAULTS = { mode: "typeset", jobs: 12, model: "gpt-6-luna", effort: "low", proofEffort: "medium" };

function parseArgs(argv) {
  const opt = { ...OCR_DEFAULTS, proofread: false };
  const rest = [];
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--proofread") opt.proofread = true;
    else if (a === "--mode") opt.mode = argv[++i];
    else if (a === "--jobs") opt.jobs = parseInt(argv[++i], 10);
    else if (a === "--model") opt.model = argv[++i];
    else if (a === "--effort") opt.effort = argv[++i];
    else if (a === "--proof-effort") opt.proofEffort = argv[++i];
    else rest.push(a);
  }
  if (rest.length !== 1 || !PROMPTS[opt.mode]) {
    console.error("usage: ocr.mjs <work> [--mode typeset|handwritten|slide] [--proofread] [--jobs N] [--effort E]");
    process.exit(2);
  }
  opt.work = path.resolve(rest[0]);
  return opt;
}

/** Transcribes (and with opt.proofread proofreads) every page image in <work>/pages; returns the failed count. */
export async function ocrPages(opt) {
  const pagesDir = path.join(opt.work, "pages");
  const pages = fs.readdirSync(pagesDir).filter((f) => /^p-\d+\.png$/.test(f)).sort();
  for (const d of ["ocr", "proof"]) fs.mkdirSync(path.join(opt.work, d), { recursive: true });
  let failed = 0;
  const timings = [];
  await pool(pages, opt.jobs, async (f) => {
    const n = f.match(/\d+/)[0].padStart(2, "0");
    const image = path.join(pagesDir, f);
    const draft = path.join(opt.work, "ocr", `${n}.md`);
    const proof = path.join(opt.work, "proof", `${n}.md`);
    const t0 = Date.now();
    try {
      if (!fs.existsSync(draft) || !mathBalanced(fs.readFileSync(draft, "utf8"))) {
        const started_at = Date.now();
        let md = "";
        for (let i = 0; i < 3 && !mathBalanced(md || "\\("); i++) md = await codexWithImage(PROMPTS[opt.mode], image, { ...opt, events: path.join(opt.work, "ocr", `${n}.events.jsonl`) });
        if (!mathBalanced(md)) throw new Error("unbalanced math delimiters after 3 tries");
        fs.writeFileSync(draft, md);
        fs.rmSync(proof, { force: true });
        timings.push({ phase: "ocr", page: n, started_at, ended_at: Date.now() });
        console.log(`ocr ${n} ${Math.round((Date.now() - t0) / 1000)}s`);
      }
      if (opt.proofread && !fs.existsSync(proof)) {
        const started_at = Date.now();
        const md = fs.readFileSync(draft, "utf8");
        const lines = visionLines(image);
        const spots = disagreements(md, lines);
        let fixed = md, log = "";
        if (spots.length) {
          const crops = path.join(opt.work, "proof", `${n}-spots`);
          fs.mkdirSync(crops, { recursive: true });
          const images = cropSpots(image, lines, spots, crops);
          const answer = await codexWithImage(proofPrompt(spots), images, { ...opt, effort: opt.proofEffort, schema: READINGS_SCHEMA, events: path.join(opt.work, "proof", `${n}.events.jsonl`) });
          [fixed, log] = applyReadings(md, spots, answer);
        }
        fs.writeFileSync(path.join(opt.work, "proof", `${n}.edits.txt`), log);
        fs.writeFileSync(proof, fixed);
        timings.push({ phase: "proof", page: n, started_at, ended_at: Date.now() });
        console.log(`proof ${n}: ${spots.length} spots, ${(log.match(/^fixed/gm) || []).length} fixed, ${Math.round((Date.now() - t0) / 1000)}s`);
      }
    } catch (e) {
      failed += 1;
      console.log(`FAILED ${n}: ${e.message}`);
    }
  });
  if (timings.length) fs.appendFileSync(path.join(opt.work, "phase-timings.jsonl"), timings.map(t => JSON.stringify(t)).join("\n") + "\n");
  console.log(`done: ${pages.length} pages, ${failed} failed`);
  return failed;
}

async function main() {
  loadEnvFile();
  process.exit(await ocrPages(parseArgs(process.argv.slice(2))) ? 1 : 0);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === fs.realpathSync(process.argv[1])) main();
