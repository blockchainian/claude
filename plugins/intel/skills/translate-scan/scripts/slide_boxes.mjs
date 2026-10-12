#!/usr/bin/env node
// ABOUTME: Builds the text boxes of an image-only slide deck for the slides pipeline: macOS Vision's line boxes,
// ABOUTME: their words corrected from a codex transcription, lines grouped into paragraph boxes, sizes in points.
//
// Usage: slide_boxes.mjs <deck.pdf> <out.json> [--work DIR] [--jobs 12]
// The work dir (default <INTEL_STATE_DIR>/translate-scan/<deck name>) holds the page images, from the PDF's own
// full-page rasters (pdfimages) or else a 150 dpi render, and the codex slide transcription, ocr/NN.md; both are
// reused on a rerun. The JSON:
// {pdf, page_size: [w, h] pt, pages: [{index, boxes: [{bbox: [x0, y0, x1, y1] pt from the top left, text (the codex
// transcription's Markdown/LaTeX for the paragraph when it matches, else Vision's), vision_text, matched (true when
// text came from the transcription),
//   font_size pt, lines: [{bbox, text}], in_figure}]}]}
// font_size is Vision's line-box height in points: on text-layer slides the true size / box height has median 1.00
// (quartiles 0.93, 1.16). in_figure marks text inside a diagram the transcription left as a figure.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { loadEnvFile, stateDir } from "../../fetch-x-mentions/scripts/env.mjs";
import { OCR_DEFAULTS, ocrPages } from "./ocr.mjs";
import { disagreements, tokens } from "./proof.mjs";
import { visionLines } from "./vision.mjs";

const FIGURE_RE = /^\[FIGURE ([\d.]+),\s*([\d.]+),\s*([\d.]+),\s*([\d.]+)\]\s*$/gm;
const BULLET_RE = /^\s*([•●▪◦‣\-–]\s*|\d+[.)]\s)/;

/** The first three words of a line, as a comparable key ("" for a shorter line). */
function opening(text) {
  const ws = tokens(text.replace(BULLET_RE, "")).map((t) => t.w.toLowerCase().replace(/[^a-z0-9]/g, "")).filter(Boolean);
  return ws.length >= 3 ? ws.slice(0, 3).join(" ") : "";
}

/** The openings of the transcription's paragraphs and bullet items: a Vision line that starts one starts a box. */
export function paragraphStarts(md) {
  return new Set(md.split("\n").filter((l) => l.trim() && !l.startsWith("[FIGURE")).map(opening).filter(Boolean));
}

/** Vision's lines with the codex transcription's spelling where the two disagree inside one line. */
export function correctedLines(lines, md) {
  const out = lines.map((l) => ({ ...l }));
  for (const s of disagreements(md, lines)) {
    if (s.lines[0] !== s.lines[1] || !s.vision) continue;
    const l = out[s.lines[0]];
    if (l.text.split(s.vision).length === 2) l.text = l.text.replace(s.vision, s.draft);
  }
  return out;
}

/** Vision sometimes cuts one text line in two at a symbol or quote, lists the pieces apart, or reads a line's tail a
 * second time: pieces on the same baseline less than a line height apart become one line again (in the position of
 * the first), and a piece lying mostly inside another is dropped. */
export function mergeRows(lines) {
  const out = lines.map((l) => ({ ...l }));
  for (let merged = true; merged;) {
    merged = false;
    for (let i = 0; i < out.length && !merged; i++)
      for (let j = 0; j < out.length && !merged; j++) {
        const p = out[i], l = out[j];
        if (i === j || p.inFigure !== l.inFigure) continue;
        const h = Math.min(p.box[3] - p.box[1], l.box[3] - l.box[1]);
        const overlap = Math.min(p.box[3], l.box[3]) - Math.max(p.box[1], l.box[1]);
        if (overlap <= 0.7 * h || l.box[0] < p.box[0]) continue;
        const inside = Math.min(p.box[2], l.box[2]) - l.box[0];
        if (inside > 0.7 * (l.box[2] - l.box[0])) {  // a duplicate read of the line's tail
          out.splice(j, 1);
          merged = true;
        } else if (l.box[0] - p.box[2] < 0.75 * h) {
          out[Math.min(i, j)] = { ...p, text: `${p.text} ${l.text}`,
            box: [p.box[0], Math.min(p.box[1], l.box[1]), l.box[2], Math.max(p.box[3], l.box[3])] };
          out.splice(Math.max(i, j), 1);
          merged = true;
        }
      }
  }
  return out;
}

/** Letters and digits only, lowercased, with o read as 0 (Vision confuses them), as character bigrams. */
function bigrams(text) {
  const t = tokens(text).map((x) => x.w).join("").toLowerCase().replace(/[^a-z0-9]/g, "").replace(/o/g, "0");
  const out = new Map();
  for (let i = 0; i < t.length - 1; i++) out.set(t.slice(i, i + 2), (out.get(t.slice(i, i + 2)) || 0) + 1);
  return [out, Math.max(0, t.length - 1)];
}

/** The transcription's paragraph for a box, when the two match (bigram Dice similarity of at least 0.75): codex's
 * text keeps subscripts, symbols and digits that Vision misreads. */
export function transcribedText(boxText, md) {
  const [a, na] = bigrams(boxText.replace(BULLET_RE, ""));
  let best = null, bestScore = 0;
  for (const line of md.split("\n")) {
    const text = line.replace(/^\s*[-*]\s+|^#+\s+/, "").trim();
    if (!text || line.startsWith("[FIGURE")) continue;
    const [b, nb] = bigrams(text);
    let common = 0;
    for (const [g, c] of a) common += Math.min(c, b.get(g) || 0);
    const score = (2 * common) / Math.max(1, na + nb);
    if (score > bestScore) [best, bestScore] = [text, score];
  }
  return bestScore >= 0.75 ? best : null;
}

/** Consecutive lines of one paragraph: a gap under half a line height, similar height, the same left edge (or a
 * hanging indent under a bullet), and the next line not opening a bullet or a paragraph of the transcription. */
export function groupLines(lines, starts = new Set()) {
  const blocks = [];
  for (const l of lines) {
    const b = blocks[blocks.length - 1];
    const prev = b && b.lines[b.lines.length - 1];
    const h = prev && prev.box[3] - prev.box[1], lh = l.box[3] - l.box[1];
    const first = b && b.lines[0];
    const aligned = first && (Math.abs(l.box[0] - first.box[0]) < 0.015
      || BULLET_RE.test(first.text) && l.box[0] > first.box[0] && l.box[0] - first.box[0] < 0.05);
    const joins = prev && l.box[1] - prev.box[3] < 0.5 * h && l.box[1] >= prev.box[1]
      && Math.abs(lh - h) < 0.4 * h && aligned && !BULLET_RE.test(l.text) && !starts.has(opening(l.text))
      && b.inFigure === l.inFigure;
    if (joins) b.lines.push(l);
    else blocks.push({ lines: [l], inFigure: l.inFigure });
  }
  return blocks;
}

function pageImages(pdf, dir) {
  fs.mkdirSync(dir, { recursive: true });
  const list = execFileSync("pdfimages", ["-list", pdf], { encoding: "utf8" }).split("\n").slice(2).filter(Boolean);
  const pages = Number(execFileSync("pdfinfo", [pdf], { encoding: "utf8" }).match(/Pages:\s+(\d+)/)[1]);
  const perPage = new Map();
  for (const row of list) perPage.set(row.trim().split(/\s+/)[0], (perPage.get(row.trim().split(/\s+/)[0]) || 0) + 1);
  const oneEach = perPage.size === pages && [...perPage.values()].every((n) => n === 1);
  if (!fs.readdirSync(dir).length)
    execFileSync(oneEach ? "pdfimages" : "pdftoppm", oneEach ? ["-png", pdf, path.join(dir, "p")]
      : ["-r", "150", "-png", pdf, path.join(dir, "p")]);
  return fs.readdirSync(dir).filter((f) => f.endsWith(".png")).sort().map((f) => path.join(dir, f));
}

async function main() {
  loadEnvFile();
  const args = process.argv.slice(2);
  const opt = { jobs: OCR_DEFAULTS.jobs, work: null };
  const rest = [];
  for (let i = 0; i < args.length; i++) {
    if (args[i] === "--work") opt.work = args[++i];
    else if (args[i] === "--jobs") opt.jobs = parseInt(args[++i], 10);
    else rest.push(args[i]);
  }
  if (rest.length !== 2) {
    console.error("usage: slide_boxes.mjs <deck.pdf> <out.json> [--work DIR] [--jobs N]");
    process.exit(2);
  }
  const [pdf, outPath] = rest.map((p) => path.resolve(p));
  const work = path.resolve(opt.work || path.join(stateDir(), "translate-scan", path.basename(pdf, ".pdf")));
  const size = execFileSync("pdfinfo", [pdf], { encoding: "utf8" }).match(/Page size:\s+([\d.]+) x ([\d.]+)/);
  const [W, H] = [Number(size[1]), Number(size[2])];
  const images = pageImages(pdf, path.join(work, "pages"));
  if (await ocrPages({ ...OCR_DEFAULTS, mode: "slide", jobs: opt.jobs, work, proofread: false })) process.exit(1);
  const pt = (b) => [b[0] * W, b[1] * H, b[2] * W, b[3] * H].map((v) => Math.round(v * 10) / 10);
  const pages = images.map((img, index) => {
    const n = path.basename(img).match(/\d+/)[0].padStart(2, "0");  // ocr.mjs names its output after the image
    const mdPath = path.join(work, "ocr", `${n}.md`);
    const md = fs.existsSync(mdPath) ? fs.readFileSync(mdPath, "utf8") : "";
    const figures = [...md.matchAll(FIGURE_RE)].map((m) => m.slice(1, 5).map(Number));
    const lines = correctedLines(visionLines(img), md).filter((l) => /[A-Za-z0-9]/.test(l.text)).map((l) => {
      const cx = (l.box[0] + l.box[2]) / 2, cy = (l.box[1] + l.box[3]) / 2;
      return { ...l, inFigure: figures.some((f) => cx > f[0] && cx < f[2] && cy > f[1] && cy < f[3]) };
    });
    const boxes = groupLines(mergeRows(lines), paragraphStarts(md)).map((b) => {
      const bb = [Math.min(...b.lines.map((l) => l.box[0])), Math.min(...b.lines.map((l) => l.box[1])),
        Math.max(...b.lines.map((l) => l.box[2])), Math.max(...b.lines.map((l) => l.box[3]))];
      const heights = b.lines.map((l) => (l.box[3] - l.box[1]) * H).sort((x, y) => x - y);
      const seen = b.lines.map((l) => l.text).join(" ");
      const matched = !b.inFigure && transcribedText(seen, md);
      return { bbox: pt(bb), text: matched || seen, vision_text: seen, matched: Boolean(matched),
        font_size: Math.round(heights[heights.length >> 1] * 10) / 10,
        lines: b.lines.map((l) => ({ bbox: pt(l.box), text: l.text })), in_figure: b.inFigure };
    });
    return { index, boxes };
  });
  fs.mkdirSync(path.dirname(outPath), { recursive: true });
  fs.writeFileSync(outPath, JSON.stringify({ pdf, page_size: [W, H], pages }, null, 1));
  console.log(`${outPath}: ${pages.length} pages, ${pages.reduce((n, p) => n + p.boxes.length, 0)} boxes`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === fs.realpathSync(process.argv[1])) main();
