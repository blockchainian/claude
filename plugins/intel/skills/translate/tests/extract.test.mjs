// ABOUTME: Tests extract.mjs: EPUB section derivation (titles, chapter numbers, grouping, back-matter latch),
// ABOUTME: fragment cleaning with image placeholders, and the zip/XML readers, on tiny generated EPUBs.

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import * as ex from "../scripts/extract.mjs";

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "extract-test-"));
let tmpCount = 0;

function tmpDir() {
  const d = path.join(tmp, String(++tmpCount));
  fs.mkdirSync(d, { recursive: true });
  return d;
}

function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) {
    c ^= b;
    for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return (c ^ 0xffffffff) >>> 0;
}

/** A zip of stored entries ({name: string|Buffer}), enough of the format for ZipFile to read. */
export function makeZip(entries) {
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const [name, value] of Object.entries(entries)) {
    const data = Buffer.isBuffer(value) ? value : Buffer.from(value, "utf8");
    const nameBuf = Buffer.from(name, "utf8");
    const crc = crc32(data);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt16LE(0x800, 6);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(data.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(nameBuf.length, 26);
    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(20, 4);
    central.writeUInt16LE(20, 6);
    central.writeUInt16LE(0x800, 8);
    central.writeUInt32LE(crc, 16);
    central.writeUInt32LE(data.length, 20);
    central.writeUInt32LE(data.length, 24);
    central.writeUInt16LE(nameBuf.length, 28);
    central.writeUInt32LE(offset, 42);
    locals.push(local, nameBuf, data);
    centrals.push(central, nameBuf);
    offset += local.length + nameBuf.length + data.length;
  }
  const cd = Buffer.concat(centrals);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(centrals.length / 2, 8);
  eocd.writeUInt16LE(centrals.length / 2, 10);
  eocd.writeUInt32LE(cd.length, 12);
  eocd.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, cd, eocd]);
}

const CONTAINER = '<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container">' +
  '<rootfiles><rootfile full-path="package.opf"/></rootfiles></container>';

/** files: {href: body}; spine: [href...] (ids == hrefs); nav: {href: title}. */
function makeEpub(file, files, spine, nav) {
  const items = Object.keys(files).map((h) => `<item id="${h}" href="${h}" media-type="application/xhtml+xml"/>`).join("");
  const opf = '<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0">' +
    '<metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>B</dc:title></metadata>' +
    `<manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>${items}</manifest>` +
    "<spine>" + spine.map((h) => `<itemref idref="${h}"/>`).join("") + "</spine></package>";
  const lis = Object.entries(nav).map(([h, t]) => `<li><a href="${h}">${t}</a></li>`).join("");
  const entries = { "META-INF/container.xml": CONTAINER, "package.opf": opf,
    "nav.xhtml": `<html><body><nav><ol>${lis}</ol></nav></body></html>` };
  for (const [h, body] of Object.entries(files)) entries[h] = `<html><body>${body}</body></html>`;
  fs.writeFileSync(file, makeZip(entries));
}

function quiet(fn) {
  const { write: out } = process.stdout;
  const { write: err } = process.stderr;
  process.stdout.write = () => true;
  process.stderr.write = () => true;
  try {
    return fn();
  } finally {
    process.stdout.write = out;
    process.stderr.write = err;
  }
}

function extractSections(work, epub) {
  quiet(() => ex.extractEpub(epub, work, false, [468, 680]));
  return JSON.parse(fs.readFileSync(path.join(work, "sections.json"), "utf8")).sections;
}

describe("titles and numbers", () => {
  test("chinese numbers", () => {
    assert.deepEqual([1, 10, 11, 20, 24, 105].map(ex.chineseNumber), ["一", "十", "十一", "二十", "二十四", "一百零五"]);
  });

  test("parse '1. Title'", () => {
    assert.deepEqual(ex.parseTitle("1. Traction Channels"), [1, "Traction Channels"]);
  });

  test("parse 'Chapter Twelve: X'", () => {
    assert.deepEqual(ex.parseTitle("Chapter Twelve: SEO"), [12, "SEO"]);
  });

  test("parse 'Chapter 7—X' (an em or en dash after the number)", () => {
    assert.deepEqual(ex.parseTitle("Chapter 7—Combinatorial Searching"), [7, "Combinatorial Searching"]);
    assert.deepEqual(ex.parseTitle("Chapter Seven – Combinatorial Searching"), [7, "Combinatorial Searching"]);
  });

  test("an ebook vendor's boilerplate page is skipped", () => {
    assert.equal(ex.classify("About This eBook", null, false), "skip");
  });

  test("parse plain title", () => {
    assert.deepEqual(ex.parseTitle("Preface: Traction Trumps Everything"), [null, "Preface: Traction Trumps Everything"]);
  });

  test("classify cover/contents/skip/chapter/front/back", () => {
    const cases = [["Cover", null, false], ["Contents", null, false], ["Index", null, true],
      ["One", 1, false], ["Preface", null, false], ["Acknowledgments", null, true]];
    assert.deepEqual(cases.map(([t, c, seen]) => ex.classify(t, c, seen)), ["cover", "contents", "skip", "chapter", "front", "back"]);
  });

  test("chapter read from 'CHAPTER n' + title headings", () => {
    assert.deepEqual(ex.chapterFromHeadings(["CHAPTER 3", "Solving Problems by Searching"]), [3, "Solving Problems By Searching"]);
  });

  test("chapter read from a single 'n Title' heading", () => {
    assert.deepEqual(ex.chapterFromHeadings(["4 Search"]), [4, "Search"]);
  });

  test("part-title regex matches a roman part, not a word starting with I/V/X", () => {
    assert.ok(ex.PART_TITLE_RE.test("II Problem Solving"));
    assert.ok(!ex.PART_TITLE_RE.test("Introduction"));
  });
});

describe("zip and xml readers", () => {
  test("ZipFile reads stored entries and raises KeyError for a missing name", () => {
    const z = new ex.ZipFile(makeZip({ "a.txt": "hello", "d/b.bin": Buffer.from([1, 2, 3]) }));
    assert.deepEqual(z.namelist(), ["a.txt", "d/b.bin"]);
    assert.equal(z.read("a.txt").toString(), "hello");
    assert.deepEqual([...z.read("d/b.bin")], [1, 2, 3]);
    assert.throws(() => z.read("nope"), ex.KeyError);
  });

  test("fromstring drops namespaces, decodes entities and keeps nested text", () => {
    const root = ex.fromstring('<?xml version="1.0"?><!DOCTYPE html><x:a xmlns:x="u" k="v &amp; w"><!-- c --><b>t&#233;<i>x</i>y</b>z</x:a>');
    assert.equal(root.tag, "a");
    assert.equal(root.attrs.k, "v & w");
    assert.equal(ex.itertext(root), "téxyz");
    assert.deepEqual([...ex.iterElements(root)].map((e) => e.tag), ["a", "b", "i"]);
  });

  test("fromstring rejects an undefined entity like ElementTree", () => {
    assert.throws(() => ex.fromstring("<a>&nbsp;</a>"), ex.ParseError);
  });

  test("pyDumps matches json.dumps: indent, floats, ascii", () => {
    assert.equal(ex.pyDumps({ a: [new ex.PyFloat(468), new ex.PyFloat(427.6)], b: null, c: "é" }, { indent: 2, ensureAscii: false }),
      '{\n  "a": [\n    468.0,\n    427.6\n  ],\n  "b": null,\n  "c": "é"\n}');
    assert.equal(ex.pyDumps({ x: 1, y: [] }), '{"x": 1, "y": []}');
    assert.equal(ex.pyDumps("é"), '"\\u00e9"');
  });
});

describe("fragments and images", () => {
  // A display equation alone in its block is a block figure; a symbol within a text line is inline.
  test("display equation alone in <p> is block", () => {
    const b1 = '<p><img src="e.png"/></p>';
    assert.equal(ex.imgInline(b1, b1.indexOf("<img")), false);
  });

  test("symbol image within a line is inline", () => {
    const b2 = '<p>当 <img src="s.png"/> 时</p>';
    assert.equal(ex.imgInline(b2, b2.indexOf("<img")), true);
  });

  test("an image the EPUB marks class=\"inline\" is inline even between punctuation", () => {
    const b3 = '<p>Pr(<em>Y</em> = <img class="inline" src="die.jpg"/>) = 1/5</p>';
    assert.equal(ex.imgInline(b3, b3.indexOf("<img")), true);
  });

  // epubFragment keeps the math/emphasis tags and turns each <img> into an ⟦IMG⟧ placeholder.
  test("epubFragment keeps tags and placeholders the image", () => {
    const work = tmpDir();
    fs.mkdirSync(path.join(work, "images"));
    const z = new ex.ZipFile(makeZip({
      "c.xhtml": '<html><body><p><strong>x</strong> is <em>a</em><sub>1</sub> <img src="i.png"/></p></body></html>',
      "i.png": Buffer.from("\x89PNG\r\n\x1a\n", "latin1"),
    }));
    const [frag, imgs, n] = ex.epubFragment(z, "c.xhtml", work, true, 0);
    assert.ok(frag.includes("<strong>x</strong>") && frag.includes("<sub>1</sub>") && frag.includes("⟦IMG:"), frag);
    assert.equal(Object.keys(imgs).length, 1);
    assert.equal(n, 1);
    assert.ok(fs.existsSync(path.join(work, "images", "e1.png")));
    // The EPUB's own file name is kept, so a book script can find a recurring glyph (a marker, a symbol) by name.
    assert.equal(imgs.e1.src, "i");
  });
});

describe("section grouping", () => {
  // A per-subsection-file EPUB groups its subsections under the parent chapter (one section, ## sub-headings).
  test("subsection grouped under its chapter (one section per chapter, not per subsection)", () => {
    const work = tmpDir();
    const epub = path.join(work, "b.epub");
    makeEpub(epub,
      { "c1.xhtml": "<p>chapter one body</p>", "c1a.xhtml": "<p>subsection one one body</p>", "c2.xhtml": "<p>chapter two body</p>" },
      ["c1.xhtml", "c1a.xhtml", "c2.xhtml"],
      { "c1.xhtml": "1. Intro", "c1a.xhtml": "1.1. First", "c2.xhtml": "2. Next" });
    const secs = extractSections(work, epub);
    assert.equal(secs.length, 2, JSON.stringify(secs.map((s) => s.outline_title)));
    assert.equal(secs[0].label, "第一章");
    assert.equal(secs[1].label, "第二章");
    assert.ok(fs.readFileSync(path.join(work, secs[0].file), "utf8").includes("subsection one one body"));
  });

  // Terminal back-matter (Notes, its untitled continuation, References, Index) after the last chapter is not
  // translated: it is dropped, not folded into the last chapter as trailing notes.
  test("terminal back-matter (notes/refs/index + untitled continuation) dropped after last chapter", () => {
    const work = tmpDir();
    const epub = path.join(work, "b.epub");
    makeEpub(epub,
      { "c1.xhtml": "<p>chapter one body</p>", "c2.xhtml": "<p>chapter two body</p>",
        "notes.xhtml": "<p>NOTE1BODY endnote text</p>", "notescont.xhtml": "<p>NOTESCONT more endnotes</p>",
        "refs.xhtml": "<p>REFSENTRY bibliography line</p>", "index.xhtml": "<p>INDEXTERM 12, 40</p>" },
      ["c1.xhtml", "c2.xhtml", "notes.xhtml", "notescont.xhtml", "refs.xhtml", "index.xhtml"],
      { "c1.xhtml": "1. Intro", "c2.xhtml": "2. Next", "notes.xhtml": "Notes", "refs.xhtml": "References", "index.xhtml": "Index" });
    const secs = extractSections(work, epub);
    const blob = secs.map((s) => fs.readFileSync(path.join(work, s.file), "utf8")).join("");
    assert.equal(secs.length, 2, JSON.stringify(secs.map((s) => [s.label, s.words])));
    for (const m of ["NOTE1BODY", "NOTESCONT", "REFSENTRY", "INDEXTERM"]) assert.ok(!blob.includes(m), m);
  });

  test("mid-book Notes between chapters does not latch away later chapters", () => {
    const work = tmpDir();
    const epub = path.join(work, "b.epub");
    makeEpub(epub,
      { "c1.xhtml": "<p>chapter one body</p>", "n1.xhtml": "<p>MIDNOTE chapter one endnotes</p>", "c2.xhtml": "<p>chapter two body survives</p>" },
      ["c1.xhtml", "n1.xhtml", "c2.xhtml"],
      { "c1.xhtml": "1. One", "n1.xhtml": "Notes", "c2.xhtml": "2. Two" });
    const secs = extractSections(work, epub);
    const blob = secs.map((s) => fs.readFileSync(path.join(work, s.file), "utf8")).join("");
    assert.equal(secs.length, 2, JSON.stringify(secs.map((s) => [s.label, s.words])));
    assert.ok(blob.includes("chapter two body survives") && !blob.includes("MIDNOTE"));
  });

  test("conclusion opens its own back section with its continuation, chapter not bloated", () => {
    const work = tmpDir();
    const epub = path.join(work, "b.epub");
    makeEpub(epub,
      { "c1.xhtml": "<p>only chapter body</p>", "concl.xhtml": "<p>CONCLUSIONBODY closing argument</p>",
        "conclcont.xhtml": "<p>CONCLCONT rest of the conclusion</p>", "index.xhtml": "<p>INDEXTERM 3, 9</p>" },
      ["c1.xhtml", "concl.xhtml", "conclcont.xhtml", "index.xhtml"],
      { "c1.xhtml": "1. Only", "concl.xhtml": "Conclusion: The End", "index.xhtml": "Index" });
    const secs = extractSections(work, epub);
    const byFile = Object.fromEntries(secs.map((s) => [s.file, fs.readFileSync(path.join(work, s.file), "utf8")]));
    const concl = secs.find((s) => s.kind === "back");
    const ch1 = secs.find((s) => s.label === "第一章");
    assert.equal(secs.length, 2, JSON.stringify(secs.map((s) => [s.label, s.kind, s.words])));
    assert.ok(concl && byFile[concl.file].includes("CONCLUSIONBODY") && byFile[concl.file].includes("CONCLCONT"));
    assert.ok(!byFile[ch1.file].includes("CONCLUSIONBODY"));
    assert.ok(!Object.values(byFile).join("").includes("INDEXTERM"));
  });

  test("answers and appendices open their own back sections; an index's untitled continuation is skipped", () => {
    const work = tmpDir();
    const epub = path.join(work, "b.epub");
    makeEpub(epub,
      { "c1.xhtml": "<p>only chapter body</p>", "ans.xhtml": "<p>ANSWERBODY 1. yes</p>",
        "ans2.xhtml": "<p>ANSCONT 2. no</p>", "appa.xhtml": "<p>APPENDIXBODY a table</p>",
        "index.xhtml": "<p>INDEXTERM 3, 9</p>", "indexa.xhtml": "<p>INDEXCONT 10, 11</p>" },
      ["c1.xhtml", "ans.xhtml", "ans2.xhtml", "appa.xhtml", "index.xhtml", "indexa.xhtml"],
      { "c1.xhtml": "1. Only", "ans.xhtml": "Answers to Exercises", "appa.xhtml": "Appendix A. Tables", "index.xhtml": "Index and Glossary" });
    const secs = extractSections(work, epub);
    const text = (s) => fs.readFileSync(path.join(work, s.file), "utf8");
    assert.deepEqual(secs.map((s) => [s.kind, s.title]),
      [["chapter", "Only"], ["back", "Answers to Exercises"], ["back", "Appendix A. Tables"]]);
    assert.ok(text(secs[1]).includes("ANSWERBODY") && text(secs[1]).includes("ANSCONT"));
    assert.ok(!text(secs[0]).includes("ANSWERBODY"));
    assert.ok(!secs.map(text).join("").includes("INDEXCONT"));
  });

  test("a section too long for one translation call splits at its numbered subsection headings", () => {
    const work = tmpDir();
    const epub = path.join(work, "b.epub");
    const words = (tag, n) => `<p>${(tag + " ").repeat(n)}</p>`;
    makeEpub(epub,
      { "c1.xhtml": "<h2>Chapter One. Basics</h2>" + words("intro", 50) + "<h3>1.1. Alpha</h3>" + words("alpha", 9000) +
          "<h4>1.1.1. Beta&#8217;s</h4>" + words("beta", 9000) + "<h3>1.2. Gamma</h3><h4>1.2.1. Delta</h4>" + words("delta", 100),
        "ans.xhtml": "<h2>Answers to Exercises</h2><h3>Section 1.1</h3>" + words("ans", 9000) + "<h4>Section 1.1.1</h4>" + words("ansb", 9000) },
      ["c1.xhtml", "ans.xhtml"],
      { "c1.xhtml": "Chapter One. Basics", "ans.xhtml": "Answers to Exercises" });
    const secs = extractSections(work, epub);
    assert.deepEqual(secs.map((s) => [s.kind, s.chapter, s.label, s.title]), [
      ["chapter", 1, "第一章", "Basics"],
      ["chapter", 1, null, "1.1 Alpha"],
      ["chapter", 1, null, "1.1.1 Beta’s"],
      ["chapter", 1, null, "1.2.1 Delta"], // "1.2 Gamma" has no body of its own: dropped, not an empty section
      ["back", null, null, "Answers to Exercises: Section 1.1"],
      ["back", null, null, "Answers to Exercises: Section 1.1.1"],
    ]);
    const text = (s) => fs.readFileSync(path.join(work, s.file), "utf8");
    assert.ok(text(secs[1]).includes("alpha") && !text(secs[1]).includes("beta"));
    assert.ok(text(secs[3]).includes("1.2. Gamma"), "a dropped heading-only section's heading leads the next one");
    assert.ok(secs[1].words >= 9000 && secs[1].words < 9010, String(secs[1].words));
  });

  test("sections.json carries the render contract: epub kind, float page size, cover, words", () => {
    const work = tmpDir();
    const epub = path.join(work, "b.epub");
    makeEpub(epub, { "c1.xhtml": "<p>chapter one body</p>" }, ["c1.xhtml"], { "c1.xhtml": "1. One" });
    extractSections(work, epub);
    const raw = fs.readFileSync(path.join(work, "sections.json"), "utf8");
    assert.ok(raw.includes('"page_size": [\n    468.0,\n    680.0\n  ]'), raw);
    const meta = JSON.parse(raw);
    assert.equal(meta.source_kind, "epub");
    assert.equal(meta.cover_image, null);
    assert.deepEqual(Object.keys(meta.sections[0]), ["id", "title", "outline_title", "chapter", "label", "kind", "start", "end", "file", "words"]);
    assert.equal(meta.sections[0].file, "text/01-one.xhtml");
  });
});
