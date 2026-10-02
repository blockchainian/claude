// ABOUTME: Labels chunk files with a bounded parallel pool; when a chunk fails, splits its
// ABOUTME: posts into parts and re-queues them in parallel, recursing to a depth cap.
//
// Why: gpt-6-luna occasionally early-stops on a chunk (returns a valid but short answer).
// Grinding more same-size retries just lengthens the tail. Instead a failed chunk is split
// (e.g. 1500 -> 3x500 -> 3x167) and the parts run in parallel: independent draws rarely all
// early-stop, and only the still-failing part recurses, so the tail stays short. Non-leaf units
// fail fast (--max-passes 1) so they split quickly; leaf units (at the depth cap, which cannot
// split further) use full passes to mop up. Labels are keyed by post id, so parts merge cleanly
// with merge-labels.mjs regardless of how a chunk was divided.
//
// Usage:
//   node run-labels.mjs --facts <facts.md> --vocab <vocab.json> --out <scratch> [--spec <spec.mjs>] \
//     [--par 20] [--split 3] [--max-depth 2] [--leaf-passes 3] <chunkN.json> ...
// Prints one line per unit (from label-codex) plus a final summary; exits non-zero if any
// posts remain unlabeled at the depth cap (their ids are listed).

import { spawn } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join, dirname, basename } from "node:path";
import { fileURLToPath } from "node:url";

// Split posts into k contiguous, roughly-even parts; drops empty tail parts.
export function splitPosts(posts, k) {
  if (k < 2 || posts.length < 2) return [posts];
  const size = Math.ceil(posts.length / k);
  const parts = [];
  for (let i = 0; i < posts.length; i += size) parts.push(posts.slice(i, i + size));
  return parts.filter((p) => p.length);
}

// Drive a work queue through a bounded parallel pool. `runOne(unit) -> Promise<bool>` labels a
// unit (true = success). On failure, a unit below maxDepth is expanded via `onSplit(unit) ->
// unit[]` and the parts re-queued; at maxDepth it is recorded as failed. Resolves { failed }.
export async function runPool(units, { par, maxDepth, runOne, onSplit }) {
  const queue = [...units];
  const failed = [];
  let active = 0;
  return new Promise((resolve) => {
    const drainQueue = () => {
      if (queue.length === 0 && active === 0) return resolve({ failed });
      while (active < par && queue.length) {
        const u = queue.shift();
        active++;
        Promise.resolve(runOne(u)).then((ok) => {
          if (!ok) {
            if (u.depth < maxDepth) queue.push(...onSplit(u));
            else failed.push(u);
          }
          active--;
          drainQueue();
        });
      }
    };
    drainQueue();
  });
}

function main() {
  const args = process.argv.slice(2);
  const opt = (k, d = null) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
  const flags = new Set(["--facts", "--vocab", "--out", "--spec", "--par", "--split", "--max-depth", "--leaf-passes"]);
  const inputs = args.filter((a, i) => !a.startsWith("--") && !flags.has(args[i - 1]));
  const facts = opt("--facts");
  const vocab = opt("--vocab");
  const out = opt("--out");
  const spec = opt("--spec");
  const par = Number(opt("--par", "20"));
  const split = Number(opt("--split", "3"));
  const maxDepth = Number(opt("--max-depth", "2"));
  const leafPasses = Number(opt("--leaf-passes", "3"));
  if (!inputs.length || !facts || !vocab || !out) {
    console.error("Usage: run-labels.mjs --facts <f> --vocab <v> --out <dir> [--spec <spec.mjs>] [--par 20] [--split 3] [--max-depth 2] <chunkN.json> ...");
    process.exit(1);
  }
  const scripts = dirname(fileURLToPath(import.meta.url));
  const labelCodex = join(scripts, "label-codex.mjs");
  let counter = 900000; // split-file numbers, above any real chunk index, so labelsN.json never collide

  const runOne = (u) =>
    new Promise((resolve) => {
      const passes = u.depth < maxDepth ? 1 : leafPasses; // non-leaf: fail fast then split; leaf: mop up
      const p = spawn(
        "node",
        [labelCodex, u.file, "--facts", facts, "--vocab", vocab, "--out", out, "--max-passes", String(passes), ...(spec ? ["--spec", spec] : [])],
        { stdio: ["ignore", "inherit", "inherit"] },
      );
      p.on("close", (code) => resolve(code === 0));
    });

  const onSplit = (u) => {
    const posts = JSON.parse(readFileSync(u.file, "utf8"));
    const parts = splitPosts(posts, split);
    if (parts.length < 2) return []; // cannot divide a single post further
    console.error(`split ${basename(u.file)} (${posts.length}) -> ${parts.map((p) => p.length).join("+")}`);
    return parts.map((part) => {
      const file = join(out, `chunk${counter++}.json`);
      writeFileSync(file, JSON.stringify(part));
      return { file, depth: u.depth + 1 };
    });
  };

  const units = inputs.map((file) => ({ file, depth: 0 }));
  runPool(units, { par, maxDepth, runOne, onSplit }).then(({ failed }) => {
    if (failed.length) {
      const ids = failed.flatMap((u) => JSON.parse(readFileSync(u.file, "utf8")).map((p) => p.id));
      console.error(`run-labels: ${failed.length} unit(s) still failed at depth ${maxDepth}; ${ids.length} posts unlabeled: ${ids.slice(0, 5).join(", ")}${ids.length > 5 ? " ..." : ""}`);
      process.exit(1);
    }
    console.log("run-labels: all units labeled");
  });
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main();
