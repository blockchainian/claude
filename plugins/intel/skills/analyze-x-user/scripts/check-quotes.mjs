#!/usr/bin/env node
// ABOUTME: Checks every quote in a profile.md against the account's archive: the status id exists, the handle
// ABOUTME: is the account's, and the quoted text is a verbatim substring of the post (whitespace and entities aside).
//
// Usage: check-quotes.mjs <profile.md> <user-dir>   (user-dir holds tweets.jsonl and replies.jsonl)
// Exits 1 and lists every bad quote; prints the count checked when all match.
import { readFileSync } from "node:fs";
import { join } from "node:path";

const norm = (s) => s.replace(/&amp;/g, "&").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/\s+/g, "");

// Every `[text](https://x.com/<handle>/status/<id>)` in the doc; returns the ones that fail, with a reason.
export function checkQuotes(doc, posts) {
  const byId = new Map(posts.map((p) => [p.id, p]));
  const bad = [];
  for (const m of doc.matchAll(/\[([^\]]+)\]\(https:\/\/x\.com\/(\w+)\/status\/(\d+)\)/g)) {
    const [, text, handle, id] = m;
    const p = byId.get(id);
    if (!p) bad.push({ id, reason: "missing", text });
    else if (handle.toLowerCase() !== (p.author || "").toLowerCase()) bad.push({ id, reason: "handle", text });
    else if (!norm(p.text || "").includes(norm(text))) bad.push({ id, reason: "text", text });
  }
  return bad;
}

function readJsonl(path) {
  try { return readFileSync(path, "utf8").split("\n").filter((l) => l.trim()).map((l) => JSON.parse(l)); } catch { return []; }
}

function main() {
  const [doc, dir] = process.argv.slice(2);
  if (!doc || !dir) { console.error("usage: check-quotes.mjs <profile.md> <user-dir>"); process.exit(1); }
  const posts = [...readJsonl(join(dir, "tweets.jsonl")), ...readJsonl(join(dir, "replies.jsonl"))];
  const text = readFileSync(doc, "utf8");
  const total = [...text.matchAll(/\]\(https:\/\/x\.com\/\w+\/status\/\d+\)/g)].length;
  const bad = checkQuotes(text, posts);
  for (const b of bad) console.log(`${b.reason.padEnd(7)} ${b.id}  ${b.text.slice(0, 60)}`);
  console.log(`${total} quotes, ${bad.length} bad`);
  process.exit(bad.length ? 1 : 0);
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split("/").pop())) main();
