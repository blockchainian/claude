#!/usr/bin/env node
// ABOUTME: Appends the labelers' chunk output (labelsN.json) into the archive's labels.jsonl, one line per
// ABOUTME: post id, so later runs label only new posts and any window can be reported without agents.
//
// Usage: merge-labels.mjs <labels.jsonl> <dir-with-labelsN.json>
import { readFileSync, writeFileSync, readdirSync, existsSync } from "node:fs";
import { join } from "node:path";
import { readLabels } from "./aggregate.mjs";

export function mergeLabels(store, dir) {
  const byId = new Map(existsSync(store) ? readLabels(store).map((l) => [l.id, l]) : []);
  const before = byId.size;
  let files = 0, replaced = 0;
  for (const f of readdirSync(dir).filter((f) => /^labels\d+\.json$/.test(f)).sort()) {
    files++;
    for (const l of JSON.parse(readFileSync(join(dir, f), "utf8"))) {
      if (byId.has(l.id)) replaced++;
      byId.set(l.id, l);
    }
  }
  writeFileSync(store, [...byId.values()].map((l) => JSON.stringify(l)).join("\n") + "\n");
  return { files, added: byId.size - before, replaced, total: byId.size };
}

function main() {
  const [store, dir] = process.argv.slice(2);
  if (!store || !dir) {
    console.error("Usage: merge-labels.mjs <labels.jsonl> <dir>");
    process.exit(1);
  }
  console.log(JSON.stringify(mergeLabels(store, dir)));
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split("/").pop())) main();
