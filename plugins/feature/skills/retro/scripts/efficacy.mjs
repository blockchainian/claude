#!/usr/bin/env node
// ABOUTME: Best-effort efficacy analysis for /feature:retro fixes.
// ABOUTME: Joins retro.json outcome records to the fixes ledger and reports waste recurrence per fix.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { parseArgs } from "node:util";

const DEFAULT_ROOT = path.join(process.env.FEATURE_STATE_DIR || path.join(os.homedir(), ".local", "state", "feature"), "retros");
const PROG = "efficacy.mjs";
const USAGE = `usage: ${PROG} [-h] [--root ROOT]\n`;
const HELP =
  USAGE +
  "\nBest-effort efficacy analysis for /feature:retro fixes.\n" +
  "\noptions:\n" +
  "  -h, --help   show this help message and exit\n" +
  "  --root ROOT  retros dir (default $FEATURE_STATE_DIR/retros, else ~/.local/state/feature/retros)\n";

const isObj = (v) => v !== null && typeof v === "object" && !Array.isArray(v);

/** Python str(): None for null/undefined, True/False for booleans. */
function pyStr(v) {
  if (v == null) return "None";
  if (typeof v === "boolean") return v ? "True" : "False";
  return String(v);
}

/** Python format(x, `.${d}f`): rounds the exact binary value half-to-even, unlike toFixed (half-up). */
function pyFixed(x, d) {
  if (Number.isNaN(x)) return "nan";
  if (!Number.isFinite(x)) return x > 0 ? "inf" : "-inf";
  const neg = x < 0 || Object.is(x, -0);
  const exact = Math.abs(x).toFixed(Math.min(100, d + 60));
  const [intPart, frac = ""] = exact.split(".");
  let digits = intPart + frac.slice(0, d);
  const rest = frac.slice(d);
  let up = false;
  if (rest.length) {
    const head = rest[0];
    const tail = rest.slice(1);
    if (head > "5") up = true;
    else if (head === "5") up = /[1-9]/.test(tail) || (Number(digits[digits.length - 1]) % 2 === 1);
  }
  if (up) {
    const arr = digits.split("");
    let i = arr.length - 1;
    while (i >= 0) {
      if (arr[i] === "9") { arr[i] = "0"; i--; } else { arr[i] = String(Number(arr[i]) + 1); break; }
    }
    if (i < 0) arr.unshift("1");
    digits = arr.join("");
  }
  const ip = d ? digits.slice(0, digits.length - d) : digits;
  const fp = d ? digits.slice(digits.length - d) : "";
  return (neg ? "-" : "") + (ip || "0") + (d ? "." + fp : "");
}

/** Python format(x, ".1%"): x * 100 formatted with one decimal and a percent sign. */
const pct1 = (x) => pyFixed(x * 100, 1) + "%";

function isDir(p) {
  try { return fs.statSync(p).isDirectory(); } catch { return false; }
}

/** Python glob's recursive `**` walk: every file under root matching `pred`, hidden dirs/files skipped, symlinked dirs followed. */
function walkFiles(root, pred, out = []) {
  if (!isDir(root)) return out;
  let entries;
  try { entries = fs.readdirSync(root, { withFileTypes: true }); } catch { return out; }
  for (const e of entries) {
    if (e.name.startsWith(".")) continue;
    const full = path.join(root, e.name);
    if (e.isDirectory() || (e.isSymbolicLink() && isDir(full))) walkFiles(full, pred, out);
    else if (pred(e.name)) out.push(full);
  }
  return out;
}

function loadRetros(root) {
  const out = [];
  for (const p of walkFiles(root, (n) => n === "retro.json").sort()) {
    try { out.push(JSON.parse(fs.readFileSync(p, "utf8"))); } catch { continue; }
  }
  return out.filter(isObj);
}

function loadFixes(root) {
  const p = path.join(root, "fixes.jsonl");
  const fixes = [];
  if (fs.existsSync(p)) {
    for (const raw of fs.readFileSync(p, "utf8").split("\n")) {
      const line = raw.trim();
      if (!line) continue;
      try { fixes.push(JSON.parse(line)); } catch { continue; }
    }
  }
  return fixes.filter(isObj);
}

/** Sum cost of findings of this class / grand_total, or null if the class is absent. */
function costShare(retro, wasteClass) {
  const gt = retro.grand_total || 0;
  const findings = Array.isArray(retro.findings) ? retro.findings : [];
  const costs = findings.filter((f) => isObj(f) && f.waste_class === wasteClass).map((f) => f.cost ?? 0);
  if (!costs.length || !gt) return null;
  return costs.reduce((a, b) => a + b, 0) / gt;
}

const mean = (xs) => xs.reduce((a, b) => a + b, 0) / xs.length;

function analyzeFix(fix, retros) {
  const wc = fix.waste_class;
  const applied = fix.applied_at || "";
  const shape = fix.shape ?? null; // optional: restrict comparison to one session shape
  const eligible = (r) => shape === null || r.shape === shape;
  const pre = retros.filter((r) => eligible(r) && (r.date || "") < applied);
  const post = retros.filter((r) => eligible(r) && (r.date || "") >= applied);
  const preShares = pre.map((r) => costShare(r, wc)).filter((s) => s !== null);
  const postShares = post.map((r) => costShare(r, wc)).filter((s) => s !== null);
  const mechanical = fix.type === "mechanical-gate";

  let verdict;
  if (!post.length) {
    verdict = "INCONCLUSIVE — no comparable post-fix session yet";
  } else if (!postShares.length) {
    verdict = mechanical
      ? "NO RECURRENCE — strong (mechanical gate: the waste is structurally blocked)"
      : "NO RECURRENCE — weak (judgment/memory fix: absence is not proof)";
  } else {
    const preM = preShares.length ? mean(preShares) : null;
    const postM = mean(postShares);
    if (preM !== null && postM < preM) {
      verdict = `REDUCED — cost-share ${pct1(preM)} → ${pct1(postM)} (recurs smaller; ${mechanical ? "gate leak" : "partial"})`;
    } else {
      const base = preM !== null ? `${pct1(preM)} → ` : "";
      verdict = `NOT EFFECTIVE — recurs at ${base}${pct1(postM)} in ${postShares.length} post-fix session(s)`;
    }
  }
  return [pre, post, preShares, postShares, verdict];
}

function usageError(msg) {
  process.stderr.write(USAGE + `${PROG}: error: ${msg}\n`);
  process.exit(2);
}

function main() {
  let parsed;
  try {
    parsed = parseArgs({
      args: process.argv.slice(2),
      allowPositionals: true,
      options: { help: { type: "boolean", short: "h" }, root: { type: "string" } },
    });
  } catch (e) {
    if (e.code === "ERR_PARSE_ARGS_UNKNOWN_OPTION") usageError(`unrecognized arguments: ${e.message.match(/'([^']+)'/)?.[1] ?? ""}`);
    if (e.code === "ERR_PARSE_ARGS_INVALID_OPTION_VALUE") {
      const opt = e.message.match(/'(-[^']+)'/)?.[1];
      usageError(opt ? `argument ${opt}: expected one argument` : e.message);
    }
    usageError(e.message);
  }
  const { values, positionals } = parsed;
  if (values.help) { process.stdout.write(HELP); return; }
  if (positionals.length) usageError(`unrecognized arguments: ${positionals.join(" ")}`);
  const root = values.root ?? DEFAULT_ROOT;
  const retros = loadRetros(root);
  const fixes = loadFixes(root);
  console.log(`retros: ${retros.length}  fixes: ${fixes.length}  (root ${root})`);
  if (!fixes.length) {
    console.log("no fixes ledger yet — nothing to score. Apply fixes and append to fixes.jsonl first.");
    return;
  }
  const byType = new Map(); // type -> [effective-ish, total]
  for (const fix of fixes) {
    const [pre, post, preS, postS, verdict] = analyzeFix(fix, retros);
    console.log(`\n[${pyStr(fix.type)}] ${pyStr(fix.fix_id)}  → waste_class=${pyStr(fix.waste_class)}  applied=${pyStr(fix.applied_at)}`);
    console.log(`  sessions: ${pre.length} pre / ${post.length} post   ` +
      `(class present: ${preS.length} pre / ${postS.length} post)`);
    console.log(`  VERDICT: ${verdict}`);
    const key = fix.type ?? null;
    if (!byType.has(key)) byType.set(key, [0, 0]);
    const t = byType.get(key);
    t[1] += 1;
    if (verdict.startsWith("NO RECURRENCE") || verdict.startsWith("REDUCED")) t[0] += 1;
  }
  console.log("\nby type (recurrence-free or reduced / total):");
  for (const [k, [good, tot]] of [...byType].sort((a, b) => (pyStr(a[0]) < pyStr(b[0]) ? -1 : pyStr(a[0]) > pyStr(b[0]) ? 1 : 0))) {
    console.log(`  ${pyStr(k)}: ${good}/${tot}`);
  }
  console.log("\nThis is recurrence evidence, not proof. Mark each fix effective/ineffective/inconclusive " +
    "by judgment; only mechanical-gate NO-RECURRENCE is near-deductive.");
}

main();
