#!/usr/bin/env node
// ABOUTME: Appends the labelers' chunk output (labelsN.json) into the archive's labels.jsonl, one line per
// ABOUTME: post id, so later runs label only new posts and any window can be reported without agents.
//
// Usage: merge-labels.mjs <labels.jsonl> <dir-with-labelsN.json> [--vocab <vocab.json>] [--min-share 0.01] [--rename topic:old=new ...]
//   --vocab   applies vocab.json's aliases to the merged labels, adds new topic / feature / interest values that
//             reach --min-share of this batch's about=true posts to vocab.json, prints the rest with counts
//   --rename  folds a value into another across the whole store (field-scoped: topic:, feature: or interest:)
//             and records it as an alias in vocab.json so later runs fold it automatically
import { readFileSync, writeFileSync, readdirSync, existsSync } from "node:fs";
import { join } from "node:path";
import { readLabels } from "./aggregate.mjs";

export const FIELDS = { topic: "topics", feature: "features", interest: "interests" };

// aliases: {"topic": {"old": "new"}, ...}; applied to every label in place.
export function applyAliases(labels, aliases = {}) {
  let n = 0;
  for (const l of labels) for (const [field, map] of Object.entries(aliases)) if (map && l[field] in map) { l[field] = map[l[field]]; n++; }
  return n;
}

export function mergeLabels(store, dir, aliases = {}) {
  const byId = new Map(existsSync(store) ? readLabels(store).map((l) => [l.id, l]) : []);
  const before = byId.size;
  let files = 0, replaced = 0;
  const batch = [];
  for (const f of readdirSync(dir).filter((f) => /^labels\d+\.json$/.test(f)).sort()) {
    files++;
    for (const l of JSON.parse(readFileSync(join(dir, f), "utf8"))) {
      if (byId.has(l.id)) replaced++;
      byId.set(l.id, l);
      batch.push(l);
    }
  }
  const renamed = applyAliases([...byId.values()], aliases);
  writeFileSync(store, [...byId.values()].map((l) => JSON.stringify(l)).join("\n") + "\n");
  return { files, added: byId.size - before, replaced, renamed, total: byId.size, batch };
}

// Values this batch wrote that vocab.json lacks: those reaching minShare of the batch's about=true posts
// are added to vocab (returned under `added`), the rest are returned under `below` with counts.
export function growVocab(batch, vocab, minShare = 0.01) {
  const about = batch.filter((l) => l.about).length;
  const added = {}, below = {};
  for (const [field, list] of Object.entries(FIELDS)) {
    vocab[list] = vocab[list] || [];
    const known = new Set(vocab[list]);
    const n = {};
    for (const l of batch) {
      const v = l[field];
      if (v && v !== "none" && !known.has(v)) n[v] = (n[v] || 0) + 1;
    }
    added[list] = []; below[list] = [];
    for (const [v, c] of Object.entries(n).sort((a, b) => b[1] - a[1])) {
      if (c >= Math.max(1, Math.ceil(about * minShare))) { vocab[list].push(v); added[list].push([v, c]); }
      else below[list].push([v, c]);
    }
  }
  return { added, below, threshold: Math.max(1, Math.ceil(about * minShare)) };
}

function main() {
  const args = process.argv.slice(2);
  const flags = ["--vocab", "--rename", "--min-share"];
  const [store, dir] = args.filter((a, i) => !a.startsWith("--") && !(i > 0 && flags.includes(args[i - 1])));
  const opt = (k) => (args.includes(k) ? args[args.indexOf(k) + 1] : null);
  const vocabPath = opt("--vocab");
  const minShare = Number(opt("--min-share") ?? 0.01);
  if (!store || !dir) {
    console.error("Usage: merge-labels.mjs <labels.jsonl> <dir> [--vocab <vocab.json>] [--min-share 0.01] [--rename topic:old=new ...]");
    process.exit(1);
  }
  const vocab = vocabPath && existsSync(vocabPath) ? JSON.parse(readFileSync(vocabPath, "utf8")) : {};
  vocab.aliases = vocab.aliases || {};
  for (const spec of args.flatMap((a, i) => (a === "--rename" ? [args[i + 1]] : []))) {
    const m = spec.match(/^(topic|feature|interest):(.+)=(.+)$/);
    if (!m) { console.error(`--rename wants topic:|feature:|interest: old=new, got ${spec}`); process.exit(1); }
    (vocab.aliases[m[1]] = vocab.aliases[m[1]] || {})[m[2]] = m[3];
    vocab[FIELDS[m[1]]] = (vocab[FIELDS[m[1]]] || []).filter((v) => v !== m[2]);
  }
  const { batch, ...r } = mergeLabels(store, dir, vocab.aliases);
  console.log(JSON.stringify(r));
  if (vocabPath) {
    const g = growVocab(batch, vocab, minShare);
    writeFileSync(vocabPath, JSON.stringify(vocab, null, 1) + "\n");
    console.log(`vocab threshold ${g.threshold} posts (${minShare * 100}% of ${batch.filter((l) => l.about).length} about-posts in this batch)`);
    for (const list of Object.values(FIELDS)) {
      console.log(`added ${list}:`, g.added[list].map(([v, n]) => `${v} ${n}`).join(", ") || "none");
      console.log(`below threshold ${list}:`, g.below[list].map(([v, n]) => `${v} ${n}`).join(", ") || "none");
    }
  }
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split("/").pop())) main();
