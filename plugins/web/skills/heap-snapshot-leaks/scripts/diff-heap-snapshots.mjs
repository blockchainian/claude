#!/usr/bin/env node
// ABOUTME: Diffs two V8 .heapsnapshot files into a ranked report of what grew and what is detached.
// ABOUTME: A leak is a constructor whose instance count and retained bytes climb, plus detached DOM nodes.
//
// Diff two heap snapshots taken around a repeated action to find web memory leaks.
//
// Take one snapshot at a clean baseline, do the suspect action N times (open and
// close a modal, navigate away and back), force GC, take a second snapshot. A leak
// is what the heap keeps that the baseline did not: a constructor whose live
// instance count and retained bytes climbed roughly in step with N, and DOM nodes
// still referenced after they left the document ("detached").
//
// Reads no browser — it parses the two JSON snapshots. Prints JSON.
// Needs only Node builtins (Node >= 18.18).
import { readFileSync, statSync } from "node:fs";
import { basename } from "node:path";
import { die, parseArgs, pyJson } from "./argparse-compat.mjs";

const DESCRIPTION = `Diff two heap snapshots taken around a repeated action to find web memory leaks.

Take one snapshot at a clean baseline, do the suspect action N times (open and
close a modal, navigate away and back), force GC, take a second snapshot. A leak
is what the heap keeps that the baseline did not: a constructor whose live
instance count and retained bytes climbed roughly in step with N, and DOM nodes
still referenced after they left the document ("detached").

Reads no browser — it parses the two JSON snapshots. Prints JSON.`;

// Return per-constructor [count, selfSize] (a Map in first-seen order), detached count, and detached-by-type.
function load(path) {
  let isFile = false;
  try { isFile = statSync(path).isFile(); } catch { /* missing */ }
  if (!isFile) die(`missing snapshot: ${path}`);
  let data;
  try {
    data = JSON.parse(readFileSync(path, "utf8"));
  } catch (exc) {
    die(`${basename(path)} is not valid JSON: ${exc.message}`);
  }
  const meta = data.snapshot.meta;
  const fields = meta.node_fields;
  const stride = fields.length;
  const fi = Object.fromEntries(fields.map((name, i) => [name, i]));
  const nodeTypes = meta.node_types[fi.type];
  const nodes = data.nodes;
  const strings = data.strings;
  const hasDetached = "detachedness" in fi;

  const byCtor = new Map();
  const detachedTypes = new Map();
  for (let base = 0; base < nodes.length; base += stride) {
    const typeName = nodeTypes[nodes[base + fi.type]];
    const name = strings[nodes[base + fi.name]];
    const size = nodes[base + fi.self_size];
    const ctor = name ? name : `(${typeName})`;
    let entry = byCtor.get(ctor);
    if (!entry) byCtor.set(ctor, (entry = [0, 0]));
    entry[0] += 1;
    entry[1] += size;
    const isDetached = name.startsWith("Detached ") || (hasDetached && nodes[base + fi.detachedness] === 2);
    if (isDetached) detachedTypes.set(ctor, (detachedTypes.get(ctor) ?? 0) + 1);
  }
  let detachedTotal = 0;
  for (const n of detachedTypes.values()) detachedTotal += n;
  return [byCtor, detachedTotal, detachedTypes];
}

function main() {
  const args = parseArgs(process.argv.slice(2), {
    description: DESCRIPTION,
    options: [
      { flag: "--before", dest: "before", type: "path", required: true, help: "baseline .heapsnapshot" },
      { flag: "--after", dest: "after", type: "path", required: true, help: ".heapsnapshot after the repeated action" },
      { flag: "--top", dest: "top", type: "int", default: 25, help: "constructors to report, by retained-byte growth" },
      { flag: "--min-size-delta", dest: "minSizeDelta", type: "int", default: 0, metavar: "MIN_SIZE_DELTA",
        help: "drop constructors whose retained-byte change is smaller than this" },
    ],
  });

  const [before, detBefore] = load(args.before);
  const [after, detAfter, detAfterTypes] = load(args.after);

  const growth = [];
  const ctors = new Set([...before.keys(), ...after.keys()]);
  for (const ctor of ctors) {
    const [cb, sb] = before.get(ctor) ?? [0, 0];
    const [ca, sa] = after.get(ctor) ?? [0, 0];
    const sizeDelta = sa - sb;
    if (Math.abs(sizeDelta) < args.minSizeDelta) continue;
    growth.push({ constructor: ctor, countBefore: cb, countAfter: ca,
                  countDelta: ca - cb, sizeDeltaBytes: sizeDelta });
  }
  // Stable sort: equal deltas keep first-seen order (before's constructors, then after-only ones).
  growth.sort((a, b) => b.sizeDeltaBytes - a.sizeDeltaBytes);

  let totalBefore = 0, totalAfter = 0, nodesBefore = 0, nodesAfter = 0;
  for (const [c, s] of before.values()) { nodesBefore += c; totalBefore += s; }
  for (const [c, s] of after.values()) { nodesAfter += c; totalAfter += s; }

  const topDetached = [...detAfterTypes.entries()].sort((a, b) => b[1] - a[1]).slice(0, 10);
  const top = growth.slice(0, args.top);
  const report = {
    totalSelfSizeDeltaBytes: totalAfter - totalBefore,
    nodeCountDelta: nodesAfter - nodesBefore,
    detached: { before: detBefore, after: detAfter, delta: detAfter - detBefore,
                topTypes: topDetached.map(([c, n]) => ({ constructor: c, count: n })) },
    growth: top,
    summary: {
      totalBytesBefore: totalBefore, totalBytesAfter: totalAfter,
      suspects: top.filter((g) => g.countDelta > 0 && g.sizeDeltaBytes > 0).slice(0, 10).map((g) => g.constructor),
    },
  };
  console.log(pyJson(report));
  return 0;
}

process.exitCode = main();
