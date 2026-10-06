#!/usr/bin/env node
// ABOUTME: Central store for source highlights - save, list and search.
// ABOUTME: One markdown file per item under items/, rebuilt index.md.

import { existsSync, mkdirSync, readdirSync, readFileSync, realpathSync, statSync, writeFileSync } from "node:fs";
import { outputDir, loadEnvFile } from "../../fetch-x-mentions/scripts/env.mjs";
loadEnvFile();
import { fileURLToPath } from "node:url";

// --- pathlib-compatible helpers: paths are kept as the user typed them (no
// --- resolving of ".."), only "." parts, repeated and trailing slashes drop.

export function pyPath(s) {
  s = String(s);
  let root = "";
  if (s.startsWith("/")) root = s.startsWith("//") && !s.startsWith("///") ? "//" : "/";
  const body = s.split("/").filter((p) => p !== "" && p !== ".").join("/");
  return root + body || ".";
}

export function pyJoin(a, b) {
  return b.startsWith("/") ? pyPath(b) : pyPath(`${a}/${b}`);
}

export function pyParent(p) {
  p = pyPath(p);
  if (p === "/" || p === "//" || p === ".") return p;
  const i = p.lastIndexOf("/");
  if (i === -1) return ".";
  if (i === 0) return "/";
  if (i === 1 && p.startsWith("//")) return "//";
  return p.slice(0, i);
}

export function pyName(p) {
  p = pyPath(p);
  if (p === "/" || p === "//" || p === ".") return "";
  return p.slice(p.lastIndexOf("/") + 1);
}

export function pyStem(p) {
  const name = pyName(p);
  const i = name.lastIndexOf(".");
  return i > 0 ? name.slice(0, i) : name;
}

// Python's str.isspace() set and str.splitlines() boundaries.
const WS = "\\t\\n\\v\\f\\r\\x1c-\\x1f \\x85\\xa0\\u1680\\u2000-\\u200a\\u2028\\u2029\\u202f\\u205f\\u3000";
const STRIP_RE = new RegExp(`^[${WS}]+|[${WS}]+$`, "g");
const LSTRIP_RE = new RegExp(`^[${WS}]+`);
const LINE_RE = new RegExp("\\r\\n|[\\n\\r\\v\\f\\x1c\\x1d\\x1e\\x85\\u2028\\u2029]");

const pyStrip = (s) => s.replace(STRIP_RE, "");
const pyLstrip = (s) => s.replace(LSTRIP_RE, "");

export function pySplitlines(text) {
  const parts = text.split(LINE_RE);
  if (parts.length && parts[parts.length - 1] === "") parts.pop();
  return parts;
}

const NON_PRINTABLE_RE = /[\p{Cc}\p{Cf}\p{Cs}\p{Co}\p{Cn}\p{Zl}\p{Zp}\p{Zs}]/u;

export function pyRepr(s) {
  const quote = s.includes("'") && !s.includes('"') ? '"' : "'";
  let out = quote;
  for (const ch of s) {
    const cp = ch.codePointAt(0);
    if (ch === "\\") out += "\\\\";
    else if (ch === quote) out += `\\${quote}`;
    else if (ch === "\n") out += "\\n";
    else if (ch === "\r") out += "\\r";
    else if (ch === "\t") out += "\\t";
    else if (cp < 0x20 || cp === 0x7f) out += `\\x${cp.toString(16).padStart(2, "0")}`;
    else if (cp < 0x7f || ch === " " || !NON_PRINTABLE_RE.test(ch)) out += ch;
    else if (cp < 0x100) out += `\\x${cp.toString(16).padStart(2, "0")}`;
    else if (cp < 0x10000) out += `\\u${cp.toString(16).padStart(4, "0")}`;
    else out += `\\U${cp.toString(16).padStart(8, "0")}`;
  }
  return out + quote;
}

// Python compares strings by code point, JS by UTF-16 code unit.
function cmpStr(a, b) {
  const ia = a[Symbol.iterator]();
  const ib = b[Symbol.iterator]();
  for (;;) {
    const x = ia.next();
    const y = ib.next();
    if (x.done && y.done) return 0;
    if (x.done) return -1;
    if (y.done) return 1;
    const d = x.value.codePointAt(0) - y.value.codePointAt(0);
    if (d) return d;
  }
}

function isFile(p) {
  try {
    return statSync(p).isFile();
  } catch {
    return false;
  }
}

function isDir(p) {
  try {
    return statSync(p).isDirectory();
  } catch {
    return false;
  }
}

const readText = (p) => readFileSync(p, "utf-8");
const writeText = (p, text) => writeFileSync(p, text, "utf-8");

export class SystemExit extends Error {}

const fail = (msg) => {
  throw new SystemExit(msg);
};

const out = [];
const print = (s = "") => out.push(`${s}\n`);
const flush = () => {
  if (out.length) process.stdout.write(out.splice(0).join(""));
};

function today() {
  const d = new Date();
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  const dd = String(d.getDate()).padStart(2, "0");
  return `${d.getFullYear()}-${mm}-${dd}`;
}

// --- the store itself ---

export function resolveRoot(env = process.env) {
  return pyJoin(outputDir(env), "digests");
}

export const ROOT = resolveRoot();
export const ITEMS = pyJoin(ROOT, "items");
export const INDEX = pyJoin(ROOT, "index.md");

export function parseFrontmatter(text) {
  if (!text.startsWith("---\n")) return [{}, text];
  const end = text.indexOf("\n---\n", 4);
  if (end === -1) return [{}, text];
  const meta = {};
  for (const line of pySplitlines(text.slice(4, end))) {
    if (line.includes(":") && !line.startsWith(" ")) {
      const i = line.indexOf(":");
      meta[pyStrip(line.slice(0, i))] = pyStrip(line.slice(i + 1)).replace(/^"+|"+$/g, "");
    }
  }
  return [meta, text.slice(end + 5)];
}

function itemFiles() {
  return readdirSync(ITEMS)
    .filter((name) => name.endsWith(".md"))
    .sort(cmpStr)
    .map((name) => ({ name, path: pyJoin(ITEMS, name) }));
}

function cmpRows(a, b) {
  for (let i = 0; i < a.length; i++) {
    const d = cmpStr(a[i], b[i]);
    if (d) return d;
  }
  return 0;
}

export function rebuildIndex() {
  const rows = [];
  for (const f of itemFiles()) {
    const [meta] = parseFrontmatter(readText(f.path));
    rows.push([
      meta.saved ?? "",
      meta.source || meta.show || "?",
      meta.title ?? pyStem(f.name),
      f.name,
      meta.url ?? "",
    ]);
  }
  rows.sort((a, b) => cmpRows(b, a));
  const lines = ["# Highlights", ""];
  for (const [saved, show, title, name, url] of rows) {
    lines.push(`- ${saved} — **${show}** — [${title}](items/${name}) — ${url}`);
  }
  writeText(INDEX, `${lines.join("\n")}\n`);
  return rows.length;
}

/** The file for this episode: its own, or the next free name beside it. */
export function freeSlot(slug, url) {
  let n = 1;
  let dest = pyJoin(ITEMS, `${slug}.md`);
  while (existsSync(dest)) {
    const [meta] = parseFrontmatter(readText(dest));
    if ((meta.url ?? "") === url) return [dest, true];
    n += 1;
    dest = pyJoin(ITEMS, `${slug}-${n}.md`);
  }
  return [dest, false];
}

export function cmdSave(draft) {
  const src = pyPath(draft);
  if (!isFile(src)) fail(`no such draft: ${src}`);
  let text = readText(src);
  const [meta] = parseFrontmatter(text);
  const missing = ["title", "url"].filter((k) => !meta[k]);
  if (missing.length) fail(`draft frontmatter is missing: ${missing.join(", ")}`);
  if (!meta.saved) {
    text = text.replace("---\n", `---\nsaved: ${today()}\n`);
    meta.saved = today();
  }
  let slug = meta.slug || (pyParent(src) === ITEMS ? pyStem(src) : pyName(pyParent(src)));
  slug = slug.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "") || "item";
  mkdirSync(ITEMS, { recursive: true });
  const [dest, existed] = freeSlot(slug, meta.url);
  if (existed) {
    print(`already saved: ${dest}`);
    print('(no-op; add take-aways with: store.mjs takeaway <file> --add "...")');
    return;
  }
  writeText(dest, text);
  const total = rebuildIndex();
  print(`saved: ${dest}`);
  print(`index: ${INDEX} (${total} items)`);
}

const TAKEAWAYS_HEADING = "## Take-aways";

/** [headingIdx, [item line indices], endIdx] or [null, [], null]. */
function readTakeaways(lines) {
  for (let i = 0; i < lines.length; i++) {
    if (pyStrip(lines[i]).toLowerCase() === TAKEAWAYS_HEADING.toLowerCase()) {
      let j = i + 1;
      const items = [];
      while (j < lines.length && !lines[j].startsWith("## ")) {
        if (pyLstrip(lines[j]).startsWith("- ")) items.push(j);
        j += 1;
      }
      return [i, items, j];
    }
  }
  return [null, [], null];
}

const renderTakeaways = (items) => [TAKEAWAYS_HEADING, "", ...items.map((it) => `- ${it}`), ""];

function firstSectionIdx(lines) {
  const i = lines.findIndex((line) => line.startsWith("## "));
  return i === -1 ? null : i;
}

function writeTakeaways(dest, items) {
  let lines = readText(dest).split("\n");
  const [start, , end] = readTakeaways(lines);
  const block = renderTakeaways(items);
  if (start !== null) {
    lines.splice(start, end - start, ...block);
  } else {
    const at = firstSectionIdx(lines);
    if (at === null) {
      lines = [...lines, ...(lines.length && pyStrip(lines[lines.length - 1]) ? [""] : []), ...block];
    } else {
      lines.splice(at, 0, ...block);
    }
  }
  writeText(dest, `${lines.join("\n").replace(/\n+$/, "")}\n`);
}

export function currentTakeaways(dest) {
  const lines = readText(dest).split("\n");
  const [, items] = readTakeaways(lines);
  return items.map((i) => pyStrip(pyLstrip(lines[i]).slice(2)));
}

function printTakeaways(items) {
  if (!items.length) {
    print("(no take-aways yet)");
    return;
  }
  items.forEach((it, n) => print(`${n + 1}. ${it}`));
}

export function cmdTakeaway(target, action, index, text) {
  const dest = pyPath(target);
  if (pyParent(dest) !== ITEMS || !isFile(dest)) {
    fail(`not a stored item: ${dest}\nsave the item first; take-aways attach to the stored file.`);
  }
  const items = currentTakeaways(dest);
  if (action === "list") {
    printTakeaways(items);
    return;
  }
  if (action === "add") {
    items.push(text);
  } else if (action === "revise") {
    if (index === null || !(index >= 1 && index <= items.length)) {
      fail(`--revise needs an item number in 1..${items.length}`);
    }
    items[index - 1] = text;
  }
  writeTakeaways(dest, items);
  print(`take-aways for ${pyName(dest)}:`);
  printTakeaways(currentTakeaways(dest));
}

const escapeRegex = (s) => s.replace(/[.*+?^${}()|[\]\\/-]/g, "\\$&");

export function cmdSearch(query) {
  if (!isDir(ITEMS)) fail(`nothing saved yet (${ITEMS} does not exist)`);
  let pattern;
  try {
    pattern = new RegExp(query, "i");
  } catch (exc) {
    print(`not a valid regex (${exc.message}) - searching for it literally`);
    pattern = new RegExp(escapeRegex(query), "i");
  }
  let hits = 0;
  for (const f of itemFiles()) {
    const text = readText(f.path);
    const [meta] = parseFrontmatter(text);
    const matches = [];
    pySplitlines(text).forEach((l, i) => {
      if (pattern.test(l)) matches.push([i + 1, pyStrip(l)]);
    });
    if (!matches.length) continue;
    hits += 1;
    print(`\n=== ${meta.source || meta.show || "?"} — ${meta.title ?? pyStem(f.name)}`);
    print(`    ${f.path}`);
    if (meta.url) print(`    ${meta.url}`);
    for (const [n, line] of matches.slice(0, 12)) {
      print(`    ${n}: ${Array.from(line).slice(0, 220).join("")}`);
    }
    if (matches.length > 12) print(`    ... ${matches.length - 12} more matches`);
  }
  print(`\n${hits} item(s) matched ${pyRepr(query)}`);
}

export function cmdList() {
  if (isFile(INDEX)) print(readText(INDEX));
  else print(`nothing saved yet (${INDEX} does not exist)`);
}

export const USAGE =
  "usage: store.mjs save <draft.md> | search <query> | list\n" +
  "       store.mjs takeaway <stored.md> --list\n" +
  '       store.mjs takeaway <stored.md> --add "<text>"\n' +
  '       store.mjs takeaway <stored.md> --revise <n> "<text>"';

export function parseTakeawayArgs(args) {
  if (!args.length) fail(USAGE);
  const [target, ...rest] = args;
  if (rest[0] === "--list") return [target, "list", null, null];
  if (rest[0] === "--add" && rest.length >= 2) return [target, "add", null, rest.slice(1).join(" ")];
  if (rest[0] === "--revise" && rest.length >= 3 && /^[0-9]+$/.test(rest[1])) {
    return [target, "revise", parseInt(rest[1], 10), rest.slice(2).join(" ")];
  }
  return fail(USAGE);
}

export function main(argv) {
  if (argv.length < 1) fail(USAGE);
  const [cmd, ...args] = argv;
  if (cmd === "save" && args.length) cmdSave(args[0]);
  else if (cmd === "takeaway") cmdTakeaway(...parseTakeawayArgs(args));
  else if (cmd === "search" && args.length) cmdSearch(args.join(" "));
  else if (cmd === "list") cmdList();
  else fail(USAGE);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === realpathSync(process.argv[1])) {
  try {
    main(process.argv.slice(2));
    flush();
  } catch (e) {
    flush();
    if (!(e instanceof SystemExit)) throw e;
    process.stderr.write(`${e.message}\n`);
    process.exitCode = 1;
  }
}
