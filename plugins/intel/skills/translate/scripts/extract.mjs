#!/usr/bin/env node
// ABOUTME: Splits an EPUB book into sections from its OPF spine (order) and nav/ncx (titles), keeping each
// ABOUTME: section's XHTML formatting (bold, emphasis, sub/superscripts, images) for translation, into a work directory.
//
// Usage: extract.mjs <book.epub> [--work <dir>] [--keep-images] [--page-size WxH]
// Writes <work>/sections.json and <work>/text/<id>-<slug>.xhtml; prints one line per section.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { inflateRawSync } from "node:zlib";

export const IMG_TOKEN = "⟦IMG:{}⟧"; // placeholder for a figure/equation stored as an image, kept verbatim through translation
export const IMG_TOKEN_RE = /⟦IMG:[^⟧]+⟧/g;
export const PROTECTED_TOKEN_RE = /⟦(?:IMG|CODE):[^⟧]+⟧/g;

export const NUMBER_WORDS = {};
["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve",
  "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen", "twenty"].forEach((w, i) => { NUMBER_WORDS[w] = i; });
for (const [tens, base] of [["twenty", 20], ["thirty", 30], ["forty", 40]]) {
  NUMBER_WORDS[tens] = base;
  for (const [w, i] of Object.entries(NUMBER_WORDS).slice(1, 10)) {
    NUMBER_WORDS[`${tens}-${w}`] = base + i;
    NUMBER_WORDS[`${tens} ${w}`] = base + i;
  }
}

const CN_DIGITS = "零一二三四五六七八九";

// Python's str.split()/strip() whitespace (Unicode), so titles tokenize exactly as the original did.
const WS = "[\\t\\n\\v\\f\\r \\x1c-\\x1f\\x85\\xa0\\u1680\\u2000-\\u200a\\u2028\\u2029\\u202f\\u205f\\u3000]";
const WS_RUN = new RegExp(`${WS}+`, "g");
const STRIP_RE = new RegExp(`^${WS}+|${WS}+$`, "g");

/** Python's str.strip(). */
export function pyStrip(s) {
  return s.replace(STRIP_RE, "");
}

/** Python's str.split() with no separator: whitespace runs split, empties dropped. */
export function pySplit(s) {
  return s.split(WS_RUN).filter(Boolean);
}

/** Python's str.title(): the first cased letter of every word upper, the rest lower. */
function pyTitle(s) {
  let out = "";
  let prevCased = false;
  for (const ch of s) {
    const cased = /[\p{Lu}\p{Ll}\p{Lt}]/u.test(ch);
    out += cased ? (prevCased ? ch.toLowerCase() : ch.toUpperCase()) : ch;
    prevCased = cased;
  }
  return out;
}

/** Python's json.dumps: ensure_ascii, indent (null for the compact ", "/": " form), and floats (PyFloat) as 468.0. */
export class PyFloat {
  constructor(value) { this.value = value; }
}

export function pyDumps(value, { indent = null, ensureAscii = true } = {}) {
  const str = (s) => {
    let j = JSON.stringify(s);
    if (ensureAscii) j = j.replace(/[\u0080-￿]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
    return j;
  };
  const walk = (v, level) => {
    if (v === null || v === undefined) return "null";
    if (v instanceof PyFloat) return Number.isInteger(v.value) ? `${v.value}.0` : String(v.value);
    if (typeof v === "boolean") return v ? "true" : "false";
    if (typeof v === "number") return String(v);
    if (typeof v === "string") return str(v);
    const items = Array.isArray(v) ? v.map((x) => walk(x, level + 1))
      : Object.entries(v).map(([k, x]) => `${str(k)}${indent === null ? ": " : ": "}${walk(x, level + 1)}`);
    const [open, close] = Array.isArray(v) ? ["[", "]"] : ["{", "}"];
    if (!items.length) return open + close;
    if (indent === null) return open + items.join(", ") + close;
    const pad = " ".repeat(indent * (level + 1));
    return `${open}\n${pad}${items.join(`,\n${pad}`)}\n${" ".repeat(indent * level)}${close}`;
  };
  return walk(value, 0);
}

/** 1 -> 一, 10 -> 十, 24 -> 二十四, 105 -> 一百零五. */
export function chineseNumber(n) {
  if (n < 10) return CN_DIGITS[n];
  if (n < 20) return "十" + (n % 10 ? CN_DIGITS[n % 10] : "");
  if (n < 100) return CN_DIGITS[Math.floor(n / 10)] + "十" + (n % 10 ? CN_DIGITS[n % 10] : "");
  const rest = n % 100;
  return CN_DIGITS[Math.floor(n / 100)] + "百" + (rest > 0 && rest < 10 ? "零" + chineseNumber(rest) : rest ? chineseNumber(rest) : "");
}

export function slugify(title) {
  const s = title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
  return s.slice(0, 40) || "section";
}

/** '1. Getting Started' / 'Chapter One: Getting Started' / 'Chapter 7—Searching' -> [1, 'Getting Started']; else
 * [null, title]. */
export function parseTitle(title) {
  const t = pyStrip(title);
  let m = t.match(/^(\d+)[.:\s]+\s*(.+)$/);
  if (m) return [parseInt(m[1], 10), pyStrip(m[2])];
  m = t.match(/^chapter\s+([a-z\- ]+?)[.:\s—–]+(.+)$/i);
  if (m && Object.hasOwn(NUMBER_WORDS, pyStrip(m[1]).toLowerCase())) return [NUMBER_WORDS[pyStrip(m[1]).toLowerCase()], pyStrip(m[2])];
  m = t.match(/^chapter\s+(\d+)[.:\s—–]+(.+)$/i);
  if (m) return [parseInt(m[1], 10), pyStrip(m[2])];
  return [null, t];
}

export function classify(title, chapter, seenChapter) {
  const t = pyStrip(title).toLowerCase();
  if (/\bcover\b/.test(t)) return "cover";
  if (t === "contents" || t === "table of contents") return "contents";
  if (["index", "notes", "endnotes", "references", "bibliography", "works cited", "further reading",
    "copyright", "title page", "half title", "also by", "about this ebook"].includes(t)) return "skip";
  if (chapter !== null) return "chapter";
  return seenChapter ? "back" : "front";
}

const PART_TITLE = /^\s*part\b/i;

/** Python's Path.stem / Path.suffix of a path's last component. */
function stemOf(p) {
  const name = path.posix.basename(p);
  const i = name.lastIndexOf(".");
  return i > 0 ? name.slice(0, i) : name;
}

function suffixOf(p) {
  const name = path.posix.basename(p);
  const i = name.lastIndexOf(".");
  return i > 0 ? name.slice(i) : "";
}

/** posixpath.normpath: collapses '.', '..' and repeated slashes. */
function normpath(p) {
  if (p === "") return ".";
  const initial = p.startsWith("/") ? (p.startsWith("//") && !p.startsWith("///") ? "//" : "/") : "";
  const out = [];
  for (const c of p.split("/")) {
    if (c === "" || c === ".") continue;
    if (c !== ".." || (!initial && (!out.length || out[out.length - 1] === "..")) ) out.push(c);
    else if (out.length) out.pop();
  }
  const joined = initial + out.join("/");
  return joined || ".";
}

/** posixpath.join(dir, href) for one component. */
function posixJoin(dir, href) {
  if (href.startsWith("/") || !dir) return href;
  return dir.endsWith("/") ? dir + href : `${dir}/${href}`;
}

/** posixpath.dirname: '' for a bare file name (where Node's gives '.'). */
function dirnameOf(p) {
  return p.includes("/") ? path.posix.dirname(p) : "";
}

/** <book dir>/.translate/<slug>, or ~/Documents/translate/<slug> when the book's folder is not writable
 * (macOS keeps this process out of some folders); the book is copied there so every later step can read it. */
export function defaultWork(book) {
  let work = path.join(path.dirname(book), ".translate", slugify(stemOf(book)));
  try {
    fs.mkdirSync(work, { recursive: true });
    return [work, book];
  } catch {
    work = path.join(os.homedir(), "Documents", "translate", slugify(stemOf(book)));
    fs.mkdirSync(work, { recursive: true });
    const copy = path.join(work, path.basename(book));
    if (!fs.existsSync(copy)) fs.writeFileSync(copy, fs.readFileSync(book));
    process.stderr.write(`${path.dirname(book)} is not writable: working in ${work} on a copy of the book\n`);
    return [work, copy];
  }
}

// --- zip reading (an EPUB is a zip of XHTML/OPF/NCX; entries are stored or deflated) ---

export class KeyError extends Error {
  constructor(name) {
    super(`"There is no item named ${JSON.stringify(name)} in the archive"`);
    this.name = "KeyError";
  }
}

/** A minimal zip reader over the central directory: read(name) -> Buffer, or KeyError when the name is absent. */
export class ZipFile {
  constructor(file) {
    this.buf = Buffer.isBuffer(file) ? file : fs.readFileSync(file);
    this.entries = new Map();
    const b = this.buf;
    let eocd = -1;
    for (let i = b.length - 22; i >= Math.max(0, b.length - 65557); i--) {
      if (b.readUInt32LE(i) === 0x06054b50) { eocd = i; break; }
    }
    if (eocd < 0) throw new Error("File is not a zip file");
    const count = b.readUInt16LE(eocd + 10);
    let p = b.readUInt32LE(eocd + 16);
    for (let n = 0; n < count; n++) {
      if (b.readUInt32LE(p) !== 0x02014b50) throw new Error("Bad magic number for central directory");
      const flags = b.readUInt16LE(p + 8);
      const method = b.readUInt16LE(p + 10);
      const csize = b.readUInt32LE(p + 20);
      const usize = b.readUInt32LE(p + 24);
      const nlen = b.readUInt16LE(p + 28);
      const elen = b.readUInt16LE(p + 30);
      const clen = b.readUInt16LE(p + 32);
      const offset = b.readUInt32LE(p + 42);
      const name = b.subarray(p + 46, p + 46 + nlen).toString(flags & 0x800 ? "utf8" : "latin1");
      this.entries.set(name, { method, csize, usize, offset });
      p += 46 + nlen + elen + clen;
    }
  }

  namelist() {
    return [...this.entries.keys()];
  }

  read(name) {
    const e = this.entries.get(name);
    if (!e) throw new KeyError(name);
    const b = this.buf;
    if (b.readUInt32LE(e.offset) !== 0x04034b50) throw new Error(`Bad magic number for file header: ${name}`);
    const start = e.offset + 30 + b.readUInt16LE(e.offset + 26) + b.readUInt16LE(e.offset + 28);
    const data = b.subarray(start, start + e.csize);
    if (e.method === 0) return Buffer.from(data);
    if (e.method === 8) return inflateRawSync(data);
    throw new Error(`compression method ${e.method} is not supported: ${name}`);
  }
}

// --- a small XML parser standing in for ElementTree: elements with local-name tag, attrs, text, children ---

export class ParseError extends Error {
  constructor(message) {
    super(message);
    this.name = "ParseError";
  }
}

const XML_ENTITIES = { amp: "&", lt: "<", gt: ">", quot: '"', apos: "'" };

function unescapeXml(s) {
  return s.replace(/&(#x[0-9a-fA-F]+|#[0-9]+|[A-Za-z_][\w.-]*);|&/g, (m, e) => {
    if (e === undefined) throw new ParseError("not well-formed (invalid token)");
    if (e[0] === "#") return String.fromCodePoint(parseInt(e[1] === "x" ? e.slice(2) : e.slice(1), e[1] === "x" ? 16 : 10));
    if (Object.hasOwn(XML_ENTITIES, e)) return XML_ENTITIES[e];
    throw new ParseError(`undefined entity &${e};`);
  });
}

/** Parses an XML document into {tag, attrs, text, children, tail} nodes; throws ParseError like ET.fromstring. */
export function fromstring(xml) {
  if (typeof xml !== "string") xml = xml.toString("utf8");
  if (xml.startsWith("﻿")) xml = xml.slice(1);
  const root = { tag: null, attrs: {}, text: "", children: [], tail: "" };
  const stack = [root];
  let i = 0;
  const addText = (t) => {
    const parent = stack[stack.length - 1];
    const text = unescapeXml(t);
    if (parent.children.length) parent.children[parent.children.length - 1].tail += text;
    else parent.text += text;
  };
  while (i < xml.length) {
    const lt = xml.indexOf("<", i);
    if (lt < 0) { addText(xml.slice(i)); break; }
    if (lt > i) addText(xml.slice(i, lt));
    if (xml.startsWith("<!--", lt)) {
      const end = xml.indexOf("-->", lt + 4);
      if (end < 0) throw new ParseError("unclosed comment");
      i = end + 3;
    } else if (xml.startsWith("<![CDATA[", lt)) {
      const end = xml.indexOf("]]>", lt + 9);
      if (end < 0) throw new ParseError("unclosed CDATA");
      const parent = stack[stack.length - 1];
      const text = xml.slice(lt + 9, end);
      if (parent.children.length) parent.children[parent.children.length - 1].tail += text;
      else parent.text += text;
      i = end + 3;
    } else if (xml.startsWith("<?", lt)) {
      const end = xml.indexOf("?>", lt + 2);
      if (end < 0) throw new ParseError("unclosed processing instruction");
      i = end + 2;
    } else if (xml.startsWith("<!", lt)) {
      let depth = 0;
      let j = lt + 2;
      for (; j < xml.length; j++) {
        const c = xml[j];
        if (c === "[") depth++;
        else if (c === "]") depth--;
        else if (c === ">" && depth <= 0) break;
      }
      if (j >= xml.length) throw new ParseError("unclosed declaration");
      i = j + 1;
    } else if (xml.startsWith("</", lt)) {
      const end = xml.indexOf(">", lt);
      if (end < 0) throw new ParseError("unclosed end tag");
      const name = xml.slice(lt + 2, end).trim();
      const el = stack.pop();
      if (!el || el.tag === null || el.qname !== name) throw new ParseError(`mismatched tag: ${name}`);
      i = end + 1;
    } else {
      const m = /^<([^\s/>]+)((?:\s+[^\s=/>]+\s*=\s*(?:"[^"]*"|'[^']*'))*)\s*(\/?)>/.exec(xml.slice(lt));
      if (!m) throw new ParseError("not well-formed (invalid token)");
      const attrs = {};
      for (const a of m[2].matchAll(/([^\s=/>]+)\s*=\s*(?:"([^"]*)"|'([^']*)')/g)) {
        attrs[a[1]] = unescapeXml(a[2] !== undefined ? a[2] : a[3]).replace(/[\t\n\r]/g, " ");
      }
      const qname = m[1];
      const el = { tag: qname.includes(":") ? qname.slice(qname.indexOf(":") + 1) : qname, qname, attrs, text: "", children: [], tail: "" };
      stack[stack.length - 1].children.push(el);
      if (!m[3]) stack.push(el);
      i = lt + m[0].length;
    }
  }
  if (stack.length !== 1) throw new ParseError("no element found");
  if (root.children.length !== 1) throw new ParseError(root.children.length ? "junk after document element" : "no element found");
  return root.children[0];
}

/** ElementTree's iter(): the element and every descendant, in document order. */
export function* iterElements(el) {
  yield el;
  for (const c of el.children) yield* iterElements(c);
}

/** ElementTree's itertext(): the element's text and every descendant's, joined. */
export function itertext(el) {
  let s = el.text;
  for (const c of el.children) s += itertext(c) + c.tail;
  return s;
}

/** Map each content file (basename, fragment dropped) to its TOC title, from the EPUB3 nav or EPUB2 ncx.
 * The first title seen for a file wins, which is the file-level entry (sub-section entries carry #fragments). */
export function epubTitles(z, navHref, ncxHref) {
  const titles = {};
  const base = (href) => path.posix.basename(href.split("#")[0]);
  const parse = (href) => {
    try {
      return fromstring(z.read(href));
    } catch (e) {
      if (e instanceof KeyError || e instanceof ParseError) return null;
      throw e;
    }
  };
  if (navHref) {
    const root = parse(navHref);
    if (root !== null) {
      for (const a of iterElements(root)) {
        if (a.tag === "a" && a.attrs.href) {
          const txt = pySplit(itertext(a)).join(" ");
          const b = base(a.attrs.href);
          if (txt && b && !Object.hasOwn(titles, b)) titles[b] = txt;
        }
      }
    }
  }
  if (!Object.keys(titles).length && ncxHref) {
    const root = parse(ncxHref);
    if (root !== null) {
      for (const np of iterElements(root)) {
        if (np.tag !== "navPoint") continue;
        let label = null;
        let content = null;
        for (const e of iterElements(np)) {
          if (label === null && e.tag === "navLabel") label = e;
          if (content === null && e.tag === "content") content = e;
        }
        if (label !== null && content !== null && content.attrs.src) {
          const txt = pySplit(itertext(label)).join(" ");
          const b = base(content.attrs.src);
          if (txt && !Object.hasOwn(titles, b)) titles[b] = txt;
        }
      }
    }
  }
  return titles;
}

/** The text of every <h1>..<h6> in a file, in order. */
export function epubHeadings(z, href) {
  let raw;
  try {
    raw = z.read(href).toString("utf8");
  } catch (e) {
    if (e instanceof KeyError) return [];
    throw e;
  }
  return [...raw.matchAll(/<h[1-6][^>]*>(.*?)<\/h[1-6]>/gis)].map((m) => pySplit(m[1].replace(/<[^>]+>/g, "")).join(" "));
}

export function epubFirstHeading(z, href) {
  const hs = epubHeadings(z, href);
  return hs.length ? hs[0] : null;
}

export const PART_TITLE_RE = /^\s*(?:part\s+)?[ivxlcdm]+\b/i; // "I …", "II …", "Part IV …" (word boundary keeps "Introduction" out)

/** A chapter number + title read from a file's headings: 'CHAPTER 3' then 'Solving Problems by Searching', or a
 * single 'N Title' heading. Used when the opener file's nav title is a part name, not the chapter title. */
export function chapterFromHeadings(headings) {
  for (let i = 0; i < headings.length; i++) {
    const h = headings[i];
    const m = h.match(/^\s*chapter\s+(\d+)\b/i);
    if (m) return [parseInt(m[1], 10), i + 1 < headings.length ? pyTitle(pyStrip(headings[i + 1])) : h];
    const [num, clean] = parseTitle(h);
    if (num !== null) return [num, clean];
  }
  return [null, null];
}

const BLOCK_TAG = /<\/?(?:p|div|li|ul|ol|table|tr|td|section|figure|figcaption|h[1-6]|br|blockquote)\b[^>]*>/i;

/** An <img> is inline (a symbol within a line of text) when the EPUB marks it class="inline", or when a word
 * character sits right before or after it WITHIN THE SAME block element; a display equation alone in its own
 * <p>/<div> is a block. Looking only up to the nearest block boundary avoids picking up text from a
 * neighbouring paragraph. */
export function imgInline(body, pos) {
  const end = body.indexOf(">", pos) + 1;
  if (/\sclass="(?:[^"]*\s)?inline(?:\s[^"]*)?"/.test(body.slice(pos, end))) return true;
  const leftParts = body.slice(Math.max(0, pos - 240), pos).split(BLOCK_TAG);
  const left = leftParts[leftParts.length - 1].replace(/<[^>]+>/g, "").replace(new RegExp(`${WS}+$`), "");
  const right = body.slice(end, end + 240).split(BLOCK_TAG)[0].replace(/<[^>]+>/g, "").replace(new RegExp(`^${WS}+`), "");
  const W = /[0-9A-Za-z一-鿿]/;
  return Boolean(left && W.test(left[left.length - 1])) || Boolean(right && W.test(right[0]));
}

/** Clean one XHTML file into a fragment for translation: body only, scripts/anchors removed, each <img>
 * replaced by an ⟦IMG:key⟧ placeholder (block on its own line, inline within the line) and the image copied
 * into <work>/images. The math and emphasis tags (strong/em/sub/sup/span) are kept for the translator. Returns
 * [fragment, {key: {file, w, h, block}}, next counter]. */
export function epubFragment(z, href, work, keepImages, counter, codes = {}) {
  const raw = z.read(href).toString("utf8");
  const m = raw.match(/<body[^>]*>(.*)<\/body>/is);
  let body = m ? m[1] : raw;
  body = body.replace(/<script.*?<\/script>/gis, "");
  body = body.replace(/<style.*?<\/style>/gis, "");
  body = body.replace(/<span\b[^>]*>|<\/span>/gi, "");
  // Numeric callouts can interrupt a Calibre code run without ending the listing.
  body = body.replace(/(<code\b[^>]*>(?:(?!<\/code>)[\s\S])*?<br\b[^>]*>(?:(?!<\/code>)[\s\S])*?)<\/code>(\s*\(\d+\))(?:<code\b[^>]*>|(<br\b[^>]*>))/gi,
    (match, code, callout, br) => code + callout + (br ? br + "</code>" : ""));
  // Protect listings before whitespace and inline-tag cleaning changes their content.
  const listing = /<pre\b[^>]*>([\s\S]*?)<\/pre>|<div\b[^>]*>\s*(?:<div\b[^>]*>\s*)?<tt\b[^>]*>([\s\S]*?)<\/tt>\s*(?:<\/div>\s*)?<\/div>|<(tt|code)\b[^>]*>((?:(?!<\/\3>)[\s\S])*?<br\b(?:(?!<\/\3>)[\s\S])*?)<\/\3>/gi;
  body = body.replace(listing, (block, pre, tt, tag, multiline) => {
    const rawCode = (pre ?? tt ?? multiline).replace(/<br\b[^>]*>/gi, "\n").replace(/<[^>]*>/g, "");
    const key = `c${++counter}`;
    codes[key] = unescapeXml(rawCode.replace(/&nbsp;/g, " ")).replace(/\u00a0/g, " ");
    return `\n<p>⟦CODE:${key}⟧</p>\n`;
  });
  body = body.replace(/<a\b[^>]*>(.*?)<\/a>/gis, "$1");
  body = body.replace(/<a\b[^>]*?\/>/gi, ""); // self-closing target anchors (<a id=.../>)
  const imgs = {};
  const hdir = dirnameOf(href);

  const before = body;
  body = body.replace(/<img\b[^>]*?\/?>/gi, (tag, offset) => {
    const src = tag.match(/src="([^"]+)"/) || tag.match(/src='([^']+)'/);
    if (!src) return "";
    counter += 1;
    const key = `e${counter}`;
    const imgHref = normpath(posixJoin(hdir, src[1]));
    const inline = imgInline(before, offset);
    if (!keepImages) return "";
    let data;
    try {
      data = z.read(imgHref);
    } catch (e) {
      if (e instanceof KeyError) return "";
      throw e;
    }
    const ext = suffixOf(imgHref) || ".png";
    fs.writeFileSync(path.join(work, "images", `${key}${ext}`), data);
    imgs[key] = { file: `${key}${ext}`, w: 0, h: 0, block: !inline, src: stemOf(imgHref) };
    const token = IMG_TOKEN.replace("{}", key);
    return inline ? token : `\n${token}\n`;
  });
  // Drop EPUB styling leftovers that would distort the page and confuse the translator, keeping their text:
  // <small> (and class="small" spans) actually shrink the rendered text, spans/anchors are noise. The math and
  // emphasis tags (strong/em/sub/sup) are kept.
  body = body.replace(/<\/?small\b[^>]*>/gi, "");
  body = body.replace(/<a\b[^>]*>|<\/a>/gi, "");
  body = body.replace(/[ \t]+/g, " ");
  body = pyStrip(body.replace(/\n{3,}/g, "\n\n"));
  return [body, imgs, counter];
}

const EPUB_SKIP_STEM = /^(cover|titlepage|halftitle|title|copyright|toc|nav|ncx|index|bibliography)\d*$/i;
// A book's terminal back-matter opens with one of these; once past the last chapter it and every spine file
// after it (continuations that carry no nav title of their own included) are skipped rather than translated.
const BACKMATTER_START = /^(notes|endnotes|references|bibliography|works cited|further reading|index)\b/i;
// A conclusion-like closing section, the answers to exercises and an appendix are real content: each opens its own
// section rather than folding into the last chapter.
const OWN_BACK_SECTION = /^(conclusion|epilogue|afterword|postscript|coda|answers|solutions|appendix|about the author|colophon)\b/i;
// One translation call handles a section of up to about this many words; a longer one (a whole chapter of a
// textbook) is split at its numbered subsection headings.
const MAX_SECTION_WORDS = 15000;
// A numbered subsection heading ("1.2.1. Mathematical Induction", "*1.2.11 ...") or an answers subsection
// ("Section 1.2.1"): the split points of an over-long section.
const SUBSECTION_HEADING_RE = /<h([2-6])\b[^>]*>([\s\S]*?)<\/h\1>/gi;
const NUMBERED_RE = /^\*?\d+(?:\.\d+)+\.?\s/;
const ANSWER_SECTION_RE = /^section\s+\d+(?:\.\d+)*$/i;

/** An over-long section split at its numbered subsection headings: the part before the first one stays the
 * section itself (it keeps the chapter label), each subsection becomes a section of the same kind and chapter
 * titled by its heading ("1.2.1 Mathematical Induction"; an answers subsection "Answers to Exercises: Section 1.2.1").
 * A heading with no body of its own ("1.2 Preliminaries" right before "1.2.1 ...") leads the next section instead
 * of making an empty one; so does a part before the first heading that has no body and no chapter label. */
export function splitSubsections(sec, wordsOf) {
  const cuts = [];
  for (const m of sec.frag.matchAll(SUBSECTION_HEADING_RE)) {
    const text = pyStrip(unescapeXml(m[2].replace(/<[^>]+>/g, "")).replace(/\s+/g, " "));
    if (NUMBERED_RE.test(text) || ANSWER_SECTION_RE.test(text)) cuts.push({ at: m.index, end: m.index + m[0].length, text });
  }
  if (!cuts.length) return [sec];
  const bodyWords = (frag) => wordsOf(frag.replace(/<h([1-6])\b[^>]*>[\s\S]*?<\/h\1>/gi, " ")); // headings aside
  const pieces = [{ ...sec, frag: sec.frag.slice(0, cuts[0].at), own: bodyWords(sec.frag.slice(0, cuts[0].at)) }];
  cuts.forEach((c, i) => {
    const frag = sec.frag.slice(c.at, i + 1 < cuts.length ? cuts[i + 1].at : undefined);
    const title = NUMBERED_RE.test(c.text) ? c.text.replace(/^(\*?\d+(?:\.\d+)+)\.?\s+/, "$1 ") : `${sec.title}: ${c.text}`;
    pieces.push({ title, outlineTitle: c.text, chapter: sec.chapter, label: null, kind: sec.kind, frag,
      own: bodyWords(frag) });
  });
  const out = [];
  let carry = "";
  pieces.forEach((p, i) => {
    const last = i === pieces.length - 1;
    if (!last && p.own === 0 && !(i === 0 && p.label)) { carry += p.frag; return; } // heading only: lead the next
    out.push({ ...p, frag: carry + p.frag });
    carry = "";
  });
  return out.map(({ own, ...p }) => p);
}

/** Split an EPUB into sections from its OPF spine (order) and nav/ncx (titles), keeping the XHTML formatting
 * that PDF extraction destroys (bold vectors, sub/superscripts, prose emphasis). Part-divider files are folded
 * into the head of the next chapter. Writes the same work-dir layout as the PDF path, with source_kind=epub. */
export function extractEpub(source, work, keepImages, pageSize) {
  const archive = new ZipFile(source);
  const fragments = new Map();
  const z = { read: href => fragments.has(href) ? Buffer.from(fragments.get(href)) : archive.read(href) };
  const cont = fromstring(z.read("META-INF/container.xml"));
  let opfPath;
  for (const el of iterElements(cont)) if (el.tag === "rootfile") { opfPath = el.attrs["full-path"]; break; }
  if (opfPath === undefined) throw new Error("StopIteration: no rootfile in META-INF/container.xml");
  const opfDir = dirnameOf(opfPath);
  const opf = fromstring(z.read(opfPath));
  const resolve = (href) => normpath(posixJoin(opfDir, href));
  const sourceStem = stemOf(source);
  const manifest = {};
  let coverId = null;
  let title = sourceStem;
  let author = "";
  let navHref = null;
  let ncxHref = null;
  for (const el of iterElements(opf)) {
    const ln = el.tag;
    const a = el.attrs;
    if (ln === "item") {
      if (!Object.hasOwn(a, "id")) throw new KeyError("id");
      manifest[a.id] = { href: resolve(a.href), media: a["media-type"] ?? "", props: a.properties ?? "" };
      if ((a.properties ?? "").includes("nav")) navHref = resolve(a.href);
      if ((a.properties ?? "").includes("cover-image")) coverId = a.id; // EPUB3 cover
      if (a["media-type"] === "application/x-dtbncx+xml") ncxHref = resolve(a.href);
    } else if (ln === "meta" && a.name === "cover") { // EPUB2 cover
      coverId = a.content ?? null;
    } else if (ln === "title" && pyStrip(el.text || "") && title === sourceStem) {
      title = pyStrip(el.text);
    } else if (ln === "creator" && pyStrip(el.text || "") && !author) {
      author = pyStrip(el.text);
    }
  }
  const spine = [];
  for (const el of iterElements(opf)) {
    if (el.tag === "itemref" && el.attrs.idref !== undefined && Object.hasOwn(manifest, el.attrs.idref)) spine.push(el.attrs.idref);
  }
  const titles = epubTitles(z, navHref, ncxHref);
  // An appendix can share a spine file with a chapter. Split sibling sections before grouping files.
  for (let i = 0; i < spine.length; i++) {
    const item = manifest[spine[i]];
    if (!item.media.startsWith("application/xhtml")) continue;
    const raw = z.read(item.href).toString("utf8");
    const body = raw.match(/<body[^>]*>([\s\S]*?)<\/body>/i);
    if (!body) continue;
    const cuts = [];
    let depth = 0;
    for (const m of body[1].matchAll(/<\/?section\b[^>]*>/gi)) {
      if (m[0].startsWith("</")) depth--;
      else {
        if (depth === 0 && pyStrip(body[1].slice(0, m.index)) && /(?:data-type|epub:type)=["']appendix["']/i.test(m[0])) cuts.push(m.index);
        depth++;
      }
    }
    if (!cuts.length) continue;
    const pieces = [0, ...cuts].map((at, n, starts) => body[1].slice(at, starts[n + 1]));
    fragments.set(item.href, `<html><body>${pieces[0]}</body></html>`);
    const ids = pieces.slice(1).map((piece, n) => {
      const id = `${spine[i]}-appendix-${n}`;
      const href = posixJoin(dirnameOf(item.href), `${stemOf(item.href)}-appendix-${n}.xhtml`);
      manifest[id] = { ...item, href };
      fragments.set(href, `<html><body>${piece}</body></html>`);
      return id;
    });
    spine.splice(i + 1, 0, ...ids);
  }
  const titleOf = (href) => {
    const raw = z.read(href).toString("utf8");
    if (/<(?:section|nav)\b[^>]*(?:data-type|epub:type)=["'](?:titlepage|copyright-page|toc)["']/i.test(raw)) {
      return /(?:data-type|epub:type)=["']toc["']/i.test(raw) ? "Contents" : "Title Page";
    }
    const heading = epubFirstHeading(z, href);
    return heading && /^appendix\b/i.test(heading) ? heading : titles[path.posix.basename(href)] || heading || stemOf(href);
  };

  fs.mkdirSync(path.join(work, "text"), { recursive: true });
  if (keepImages) fs.mkdirSync(path.join(work, "images"), { recursive: true });
  const sections = [];
  const images = {};
  const codes = {};
  let pendingPrefix = "";
  let seenChapter = false;
  let counter = 0;
  let idx = 0;
  let cur = null; // the chapter/section being accumulated; subsections and trailing notes append to it

  const wordsOf = (frag) => pySplit(frag.replace(PROTECTED_TOKEN_RE, " ").replace(/<[^>]+>/g, " ")).length;

  const emit = (sec) => {
    idx += 1;
    const sid = String(idx).padStart(2, "0");
    const sfile = `text/${sid}-${slugify(sec.title)}.xhtml`;
    fs.writeFileSync(path.join(work, sfile), sec.frag);
    sections.push({ id: sid, title: sec.title, outline_title: sec.outlineTitle, chapter: sec.chapter, label: sec.label,
      kind: sec.kind, start: 0, end: 0, file: sfile, words: wordsOf(sec.frag) });
  };

  const flush = () => {
    if (cur === null) return;
    for (const sec of wordsOf(cur.frag) > MAX_SECTION_WORDS ? splitSubsections(cur, wordsOf) : [cur]) emit(sec);
    cur = null;
  };

  const chapterAt = (idref) => { // the chapter number this spine file opens, or null. Must track the main loop's
    // chapter resolution below (parseTitle + part-opener headings); if that changes, change this too, or
    // lastChapterPos drifts and the back-matter latch arms on the wrong file.
    const item = manifest[idref];
    if (!item.media.startsWith("application/xhtml")) return null;
    const stitle = titleOf(item.href);
    let [chap] = parseTitle(stitle);
    if (chap === null && !(PART_TITLE.test(stitle)) && PART_TITLE_RE.test(stitle)) {
      chap = chapterFromHeadings(epubHeadings(z, item.href))[0];
    }
    return chap;
  };

  // Arm the back-matter latch only past the last chapter, so a per-chapter "Notes" between chapters still passes.
  let lastChapterPos = -1;
  spine.forEach((idref, i) => { if (chapterAt(idref) !== null) lastChapterPos = i; });
  let inBackmatter = false;
  let skipping = false; // the last file was skipped (an index, a bibliography): its untitled continuations are too

  for (let pos = 0; pos < spine.length; pos++) {
    const item = manifest[spine[pos]];
    if (!item.media.startsWith("application/xhtml")) continue;
    if (inBackmatter) continue; // the book's terminal back-matter has started — skip it and everything after
    const href = item.href;
    const stem = stemOf(path.posix.basename(href));
    const navTitled = Object.hasOwn(titles, path.posix.basename(href));
    if (skipping && !navTitled && !epubFirstHeading(z, href)) continue;
    skipping = false;
    const stitle = titleOf(href);
    if (pos > lastChapterPos && BACKMATTER_START.test(pyStrip(stitle))) {
      inBackmatter = true;
      continue;
    }
    if (/^(?:[ivxlcdm]+|\d+)$/i.test(pyStrip(stitle))) continue; // a bare page number/roman: a series-ad or filler page
    let [chapter, clean] = parseTitle(stitle);
    let isSubsection = /^\s*\d+(?:\.\d+)+/.test(stitle); // 1.1, 2.10 … a subsection, not a chapter
    const isPart = PART_TITLE.test(stitle);
    if (EPUB_SKIP_STEM.test(stem)) { skipping = true; continue; }
    if (chapter === null && !isPart && PART_TITLE_RE.test(stitle)) {
      // First chapter of a part: this opener file's nav title is the PART name, so the chapter number and
      // title live in its headings ("CHAPTER 3" / "Solving Problems by Searching"). See translate-roman parts.
      const [hnum, htitle] = chapterFromHeadings(epubHeadings(z, href));
      if (hnum !== null) { chapter = hnum; clean = htitle; isSubsection = false; }
    }
    let frag, secImgs;
    [frag, secImgs, counter] = epubFragment(z, href, work, keepImages, counter, codes);
    if (isPart && chapter === null) { // a part divider folds into the head of the next chapter
      pendingPrefix += frag + "\n\n";
      Object.assign(images, secImgs);
      continue;
    }
    const kind = classify(stitle, chapter, seenChapter);
    if (kind === "cover" || kind === "contents" || kind === "skip") { skipping = kind === "skip"; continue; }
    // A subsection (1.1 …) or a chapter's trailing notes (its Bibliographical Remarks etc.) belong under the
    // current chapter, becoming ## sub-headings, so a per-subsection-file EPUB groups into one section per
    // chapter like a per-chapter-file one. A Conclusion/Epilogue/Afterword is substantial standalone content,
    // so it opens its own section instead — its own untitled continuation files then append to it.
    const isOwnBack = kind === "back" && OWN_BACK_SECTION.test(clean || stitle);
    if (cur !== null && (isSubsection || (kind === "back" && !isOwnBack))) {
      cur.frag += "\n\n" + frag;
      Object.assign(images, secImgs);
      continue;
    }
    flush(); // a new top-level section starts: emit the accumulated one
    if (kind === "chapter") seenChapter = true;
    Object.assign(images, secImgs);
    const bodyFrag = kind === "chapter" && pendingPrefix ? pendingPrefix + frag : frag;
    pendingPrefix = kind === "chapter" ? "" : pendingPrefix;
    cur = { title: clean, outlineTitle: stitle, chapter, label: chapter ? `第${chineseNumber(chapter)}章` : null, kind, frag: bodyFrag };
  }
  flush();

  const nums = sections.filter((s) => s.label).map((s) => s.chapter); // chapter openers; split subsections share the number
  if (nums.length) { // a gap or duplicate means an opener was not recognised — the general safety net for a new EPUB layout
    const gaps = [];
    for (let n = Math.min(...nums); n <= Math.max(...nums); n++) if (!nums.includes(n)) gaps.push(n);
    const dupes = [...new Set(nums.filter((n) => nums.filter((x) => x === n).length > 1))].sort((a, b) => a - b);
    if (gaps.length) process.stderr.write(`warn: chapter numbers skip [${gaps.join(", ")}] — an opener may be mislabelled\n`);
    if (dupes.length) process.stderr.write(`warn: chapter numbers repeat [${dupes.join(", ")}] — subsections may not be grouping\n`);
  }

  let coverFile = null;
  if (coverId && Object.hasOwn(manifest, coverId)) {
    const ch = manifest[coverId];
    let imgHref = ch.href;
    if (!ch.media.startsWith("image")) {
      const craw = z.read(ch.href).toString("utf8");
      const mm = craw.match(/<img[^>]+src=["']([^"']+)["']/);
      imgHref = mm ? normpath(posixJoin(dirnameOf(ch.href), mm[1])) : null;
    }
    if (imgHref) {
      try {
        const ext = suffixOf(imgHref) || ".jpg";
        fs.writeFileSync(path.join(work, `cover${ext}`), z.read(imgHref));
        coverFile = `cover${ext}`;
      } catch (e) {
        if (!(e instanceof KeyError)) throw e;
        coverFile = null;
      }
    }
  }

  const meta = { source: String(source), source_kind: "epub", title, author,
    page_size: pageSize.map((x) => new PyFloat(x)), cover_image: coverFile, sections };
  fs.writeFileSync(path.join(work, "sections.json"), pyDumps(meta, { indent: 2, ensureAscii: false }));
  fs.writeFileSync(path.join(work, "codeblocks.json"), pyDumps(codes, { indent: 1, ensureAscii: false }));
  if (keepImages) {
    fs.writeFileSync(path.join(work, "images.json"), pyDumps(images, { indent: 1, ensureAscii: false }));
    process.stderr.write(`kept ${Object.keys(images).length} images in ${path.join(work, "images")}\n`);
  }
  for (const s of sections) {
    process.stdout.write(`${s.id} ${s.kind.padEnd(8)} ${s.outline_title}: ${s.words} words -> ${s.file}\n`);
  }
  process.stderr.write(`cover: ${coverFile === null ? "None" : coverFile}; page size: ${pyFloatStr(pageSize[0])}x${pyFloatStr(pageSize[1])}pt\n`);
  process.stdout.write(`work dir: ${work}\n`);
}

/** Python's str(float). */
function pyFloatStr(x) {
  return Number.isInteger(x) ? `${x}.0` : String(x);
}

const USAGE = "usage: extract.mjs [-h] [--work WORK] [--keep-images] [--page-size PAGE_SIZE] book\n";

function usageError(message) {
  process.stderr.write(USAGE + `extract.mjs: error: ${message}\n`);
  process.exit(2);
}

/** argparse-compatible: positional book, --work, --keep-images, --page-size (also --flag=value). */
export function parseArgs(argv) {
  const args = { book: null, work: null, keepImages: false, pageSize: "468x680" };
  const positional = [];
  for (let i = 0; i < argv.length; i++) {
    let a = argv[i];
    if (a === "-h" || a === "--help") {
      process.stdout.write(USAGE + "\npositional arguments:\n  book\n\noptions:\n  -h, --help            show this help message and exit\n" +
        "  --work WORK\n  --keep-images         keep figures and equations that are stored as images: extract each section's text\n" +
        "                        with ⟦IMG:key⟧ placeholders at the image positions and copy the images into <work>/images\n" +
        "  --page-size PAGE_SIZE\n                        rendered page size in pt as WxH (default 468x680, a 6.5x9.4in trade book)\n");
      process.exit(0);
    }
    if (a === "--") { positional.push(...argv.slice(i + 1)); break; }
    if (a.startsWith("--")) {
      let value = null;
      if (a.includes("=")) { value = a.slice(a.indexOf("=") + 1); a = a.slice(0, a.indexOf("=")); }
      if (a === "--keep-images") { if (value !== null) usageError("argument --keep-images: ignored explicit argument '" + value + "'"); args.keepImages = true; continue; }
      if (a === "--work" || a === "--page-size") {
        if (value === null) { if (i + 1 >= argv.length) usageError(`argument ${a}: expected one argument`); value = argv[++i]; }
        if (a === "--work") args.work = value; else args.pageSize = value;
        continue;
      }
      usageError(`unrecognized arguments: ${a}`);
    }
    positional.push(a);
  }
  if (!positional.length) usageError("the following arguments are required: book");
  if (positional.length > 1) usageError(`unrecognized arguments: ${positional.slice(1).join(" ")}`);
  args.book = positional[0];
  return args;
}

export function main(argv) {
  const args = parseArgs(argv);
  const source = path.resolve(args.book);
  if (suffixOf(source).toLowerCase() !== ".epub") {
    process.stderr.write(`extract.mjs takes an EPUB; got ${path.basename(source)}. Download the book's EPUB with the download-book skill ` +
      "and pass that.\n");
    process.exit(2);
  }
  let work;
  if (args.work) {
    work = path.resolve(args.work);
    fs.mkdirSync(work, { recursive: true });
  } else {
    [work] = defaultWork(source);
  }

  const pageSize = args.pageSize.toLowerCase().split("x").map((x) => {
    const f = Number(x);
    if (x.trim() === "" || Number.isNaN(f)) throw new Error(`ValueError: could not convert string to float: '${x}'`);
    return Math.round(f * 100) / 100;
  });
  extractEpub(source, work, args.keepImages, pageSize);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === fs.realpathSync(process.argv[1])) main(process.argv.slice(2));
