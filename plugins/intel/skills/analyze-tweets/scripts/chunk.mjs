#!/usr/bin/env node
// ABOUTME: Splits the whole clean corpus into fixed-size chunk files for the labeling subagents,
// ABOUTME: keeping only the fields a labeler needs; every post lands in exactly one chunk.
//
// Usage: chunk.mjs <clean.json> --size 300 --out <dir>
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";

export function slim(t) {
  return {
    id: t.id,
    author: t.author,
    likes: t.likes,
    replies: t.replies,
    date: (t.created_at || "").slice(4, 10),
    lang: t.lang,
    text: t.text.replace(/\s+/g, " ").slice(0, 600),
  };
}

export function chunk(tweets, size) {
  const out = [];
  for (let i = 0; i < tweets.length; i += size) out.push(tweets.slice(i, i + size).map(slim));
  return out;
}

function main() {
  const args = process.argv.slice(2);
  const input = args.find((a) => !a.startsWith("--"));
  const size = args.includes("--size") ? Number(args[args.indexOf("--size") + 1]) : 300;
  const out = args[args.indexOf("--out") + 1];
  if (!input || !out) {
    console.error("Usage: chunk.mjs <clean.json> --size N --out <dir>");
    process.exit(1);
  }
  mkdirSync(out, { recursive: true });
  const chunks = chunk(JSON.parse(readFileSync(input, "utf8")), size);
  chunks.forEach((c, i) => writeFileSync(join(out, `chunk${i}.json`), JSON.stringify(c)));
  console.log(`${chunks.length} chunks of ${size} -> ${out}/chunk0..${chunks.length - 1}.json`);
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split("/").pop())) main();
