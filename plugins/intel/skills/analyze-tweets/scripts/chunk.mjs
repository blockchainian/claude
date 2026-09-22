#!/usr/bin/env node
// ABOUTME: Splits the clean posts that have no label yet into fixed-size chunk files for the labeling
// ABOUTME: subagents, keeping only the fields a labeler needs; every unlabeled post lands in exactly one chunk.
//
// Usage: chunk.mjs <clean.json> --size 2000 --out <dir> [--labels <labels.jsonl>]
import { readFileSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import { join } from "node:path";
import { readLabels } from "./aggregate.mjs";

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

export function chunk(tweets, size, labeled = new Set()) {
  const todo = tweets.filter((t) => !labeled.has(t.id));
  const out = [];
  for (let i = 0; i < todo.length; i += size) out.push(todo.slice(i, i + size).map(slim));
  return out;
}

function main() {
  const args = process.argv.slice(2);
  const input = args.find((a) => !a.startsWith("--"));
  const size = args.includes("--size") ? Number(args[args.indexOf("--size") + 1]) : 2000;
  const out = args[args.indexOf("--out") + 1];
  const store = args.includes("--labels") ? args[args.indexOf("--labels") + 1] : null;
  if (!input || !out) {
    console.error("Usage: chunk.mjs <clean.json> --size N --out <dir> [--labels <labels.jsonl>]");
    process.exit(1);
  }
  mkdirSync(out, { recursive: true });
  const labeled = new Set(store && existsSync(store) ? readLabels(store).map((l) => l.id) : []);
  const tweets = JSON.parse(readFileSync(input, "utf8"));
  const chunks = chunk(tweets, size, labeled);
  chunks.forEach((c, i) => {
    writeFileSync(join(out, `chunk${i}.json`), JSON.stringify(c));
    writeFileSync(join(out, `chunk${i}.tsv`), c.map((t) => [t.id, t.author, t.likes, t.date, t.lang, t.text.replace(/\t/g, " ")].join("\t")).join("\n") + "\n");
  });
  const n = chunks.reduce((a, c) => a + c.length, 0);
  console.log(`${n} unlabeled of ${tweets.length} posts (${tweets.length - n} already in ${store ?? "no store"}) -> ${chunks.length} chunks of ${size} in ${out}`);
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split("/").pop())) main();
