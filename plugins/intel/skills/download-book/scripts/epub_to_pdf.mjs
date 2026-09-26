// ABOUTME: Convert a book EPUB into a dark-themed English PDF (Calibre + pdf-lib).
// ABOUTME: Typesets in Baskerville on a #000409 page, then verifies the result.

import { PDFDocument, PDFName, PDFArray } from 'pdf-lib';
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync, existsSync, rmSync, mkdtempSync, readdirSync } from 'node:fs';
import { basename, extname, join, dirname } from 'node:path';
import os from 'node:os';

// Page/background/text spec follows the English PDFs the format is modelled on.
const BG = [0.0, 0.0157, 0.0353]; // #000409
// One color for all text (body, headings, footnotes); !important beats the
// EPUB's own heading styles, which otherwise stay near-black and vanish on the
// dark page.
const EXTRA_CSS =
  'html{background:#000409;-webkit-print-color-adjust:exact;print-color-adjust:exact}' +
  ' *{color:#606e6a !important}' +
  ' body{text-align:justify;line-height:1.5} p{text-indent:2em}';

function ebookConvertBin() {
  for (const c of ['ebook-convert', '/Applications/calibre.app/Contents/MacOS/ebook-convert']) {
    try { execFileSync(c, ['--version'], { stdio: 'ignore' }); return c; } catch {}
  }
  throw new Error('ebook-convert (Calibre) not found; install Calibre');
}

function tryRun(cmd, args) {
  try { return execFileSync(cmd, args, { encoding: 'utf8', maxBuffer: 1 << 28 }); }
  catch { return null; }
}

// Fraction of pixels within tolerance of the dark background, from a low-res PPM render.
function darkFraction(pdf, page) {
  const dir = mkdtempSync(join(os.tmpdir(), 'e2p-ppm-'));
  const prefix = join(dir, 'p');
  tryRun('pdftoppm', ['-f', String(page), '-l', String(page), '-r', '12', pdf, prefix]);
  const file = readdirSync(dir).map((f) => join(dir, f)).find((f) => f.endsWith('.ppm'));
  if (!file) { rmSync(dir, { recursive: true, force: true }); return null; }
  const buf = readFileSync(file);
  rmSync(dir, { recursive: true, force: true });
  // Parse binary PPM (P6): magic, width, height, maxval, then RGB bytes.
  let pos = 0;
  const token = () => {
    while (pos < buf.length && /\s/.test(String.fromCharCode(buf[pos]))) pos++;
    let s = '';
    while (pos < buf.length && !/\s/.test(String.fromCharCode(buf[pos]))) s += String.fromCharCode(buf[pos++]);
    return s;
  };
  if (token() !== 'P6') return null;
  const w = +token(), h = +token(); token(); pos++; // skip single whitespace after maxval
  let dark = 0; const total = w * h;
  for (let i = pos; i + 2 < buf.length; i += 3) {
    if (buf[i] < 20 && buf[i + 1] < 30 && buf[i + 2] < 45) dark++;
  }
  return total ? dark / total : null;
}

const epub = process.argv[2];
let out = process.argv[3];
if (!epub) { console.error('usage: epub_to_pdf.mjs <book.epub> [out.pdf]'); process.exit(2); }
if (!existsSync(epub)) { console.error('epub not found: ' + epub); process.exit(2); }
if (!out) out = join(dirname(epub), basename(epub, extname(epub)) + '.pdf');

// Step 1 — Calibre lays out the EPUB into a PDF (clickable TOC + bookmarks).
const bin = ebookConvertBin();
const tmpDir = mkdtempSync(join(os.tmpdir(), 'e2p-'));
const calibrePdf = join(tmpDir, 'book.calibre.pdf');
execFileSync(bin, [
  epub, calibrePdf,
  '--custom-size', '5.93x9.153', '--unit', 'inch',
  '--pdf-page-margin-left', '41', '--pdf-page-margin-right', '41',
  '--pdf-page-margin-top', '40', '--pdf-page-margin-bottom', '40',
  '--pdf-serif-family', 'Baskerville', '--pdf-default-font-size', '16',
  '--embed-all-fonts', '--preserve-cover-aspect-ratio', '--pdf-add-toc',
  '--extra-css', EXTRA_CSS,
], { stdio: ['ignore', 'ignore', 'inherit'] });

// Step 2 — paint the dark page background behind every page except the cover.
const doc = await PDFDocument.load(readFileSync(calibrePdf));
const pages = doc.getPages();
for (let i = 1; i < pages.length; i++) {
  const { width, height } = pages[i].getSize();
  const op = `q ${BG[0]} ${BG[1].toFixed(4)} ${BG[2].toFixed(4)} rg 0 0 ${width} ${height} re f Q\n`;
  const ref = doc.context.register(doc.context.stream(op));
  const node = pages[i].node;
  const contents = node.get(PDFName.of('Contents'));
  const arr = PDFArray.withContext(doc.context);
  arr.push(ref); // prepend so the fill sits behind the text
  if (contents instanceof PDFArray) for (const e of contents.asArray()) arr.push(e);
  else if (contents) arr.push(contents);
  node.set(PDFName.of('Contents'), arr);
}
writeFileSync(out, await doc.save());
rmSync(tmpDir, { recursive: true, force: true });

// Step 3 — verify the result objectively.
const fonts = tryRun('pdffonts', [out]) || '';
const baskervilleEmbedded = fonts.split('\n')
  .some((l) => /baskerville/i.test(l) && /\byes\b/.test(l));
const textPage = Math.min(pages.length, 8);
const textDark = darkFraction(out, textPage);
const coverDark = darkFraction(out, 1);
let outlinePresent = false, tocLinks = 0;
try { outlinePresent = !!doc.catalog.get(PDFName.of('Outlines')); } catch {}
for (const p of pages.slice(0, 60)) {
  const annots = p.node.get(PDFName.of('Annots'));
  if (!(annots instanceof PDFArray)) continue;
  for (const a of annots.asArray()) {
    const dict = doc.context.lookup(a);
    const sub = dict?.get?.(PDFName.of('Subtype'));
    if (sub === PDFName.of('Link') && (dict.get(PDFName.of('Dest')) || dict.get(PDFName.of('A')))) tocLinks++;
  }
}

const checks = {
  baskerville_embedded: baskervilleEmbedded,
  text_page_dark: textDark == null ? null : +textDark.toFixed(3),
  cover_not_darkened: coverDark == null ? null : coverDark < 0.2,
  outline_present: outlinePresent,
  toc_links_front: tocLinks,
};
const ok = baskervilleEmbedded && textDark != null && textDark > 0.6
  && coverDark != null && coverDark < 0.2 && outlinePresent && tocLinks > 0;

console.log(JSON.stringify({ epub, pdf: out, pages: pages.length, ok, checks }, null, 2));
