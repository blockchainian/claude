#!/usr/bin/env node
// ABOUTME: Labels one chunk with a single no-tool codex exec call in a private CODEX_HOME: the rules are the
// ABOUTME: model's instructions, the facts and posts the prompt, and the JSON answer is checked against the chunk.
//
// Usage: label-codex.mjs <chunkN.json> --facts <facts.md> --vocab <vocab.json> --out <dir> [--spec <spec.mjs>] [--model gpt-6-luna] [--effort low] [--service-tier priority]
// --spec names the module holding the rules, fields and schema (default: mentions-spec.mjs, an app's mentions).
// Defaults to the Fast service tier ("priority": 1.5x speed, 2x price, still ~1/10 the cost of gpt-6-sol);
// pass --service-tier standard to opt out. An unsupported tier silently downgrades to standard.
// Writes labelsN.json (N from the chunk file name) plus labelsN.events.jsonl and prints one line of usage.
import { readFileSync, writeFileSync, mkdirSync, mkdtempSync, copyFileSync, existsSync } from "node:fs";
import { join, basename } from "node:path";
import { tmpdir, homedir } from "node:os";
import { spawnSync } from "node:child_process";
import { pathToFileURL } from "node:url";
import * as mentions from "./mentions-spec.mjs";

// A spec module exports RULES, FIELDS, SCHEMA, fields(vocab), VOCAB and isNoise; the mentions spec is the default.
export const loadSpec = (path) => (path ? import(pathToFileURL(path).href) : Promise.resolve(mentions));

export function buildPrompt(appFacts, posts, vocab = {}, spec = mentions) {
  const lines = posts.map((t, i) => [i + 1, t.author, t.likes, t.date, t.lang, t.text.replace(/\t/g, " ")].join("\t"));
  return `${appFacts}\n${spec.fields(vocab)}\n${posts.length} posts:\n${lines.join("\n")}\n`;
}

export function parseLabels(message, posts, spec = mentions) {
  const labels = JSON.parse(message).labels;
  const unknown = labels.filter((l) => !Number.isInteger(l.n) || l.n < 1 || l.n > posts.length).map((l) => l.n);
  if (unknown.length) throw new Error(`${unknown.length} unknown post numbers: ${unknown.slice(0, 3).join(", ")}`);
  const got = new Set(labels.map((l) => l.n));
  const missing = posts.map((_, i) => i + 1).filter((n) => !got.has(n));
  if (missing.length) throw new Error(`missing ${missing.length} posts: ${missing.slice(0, 3).join(", ")}`);
  if (got.size !== labels.length) throw new Error(`${labels.length - got.size} duplicate post numbers`);
  labels.forEach((l, i) => { if (l.n !== i + 1) throw new Error(`order differs at ${i}: ${l.n}`); });
  return labels.map((l, i) => Object.fromEntries(spec.FIELDS.map((f) => [f, f === "id" ? posts[i].id : l[f]])));
}

// Every valid, in-range, first-seen label by post number. Lenient: a truncated answer (the model stops
// numbering partway) or un-parseable output yields whatever came back, never a throw, so the caller can
// re-ask for just the gap.
export function collectLabels(message, count) {
  let arr;
  try { arr = JSON.parse(message).labels; } catch { return new Map(); }
  if (!Array.isArray(arr)) return new Map();
  const byN = new Map();
  for (const l of arr) {
    if (!Number.isInteger(l?.n) || l.n < 1 || l.n > count) continue;
    if (!byN.has(l.n)) byN.set(l.n, l);
  }
  return byN;
}

// Label every post, re-asking only for the ones still missing (a shorter prompt the model rarely truncates),
// up to maxPasses. Throws when a post never comes back, so a partial answer is never written as if complete.
// callLabeler(subPosts) -> { message, usage, seconds, tools }.
export function labelWithRetry(posts, callLabeler, { maxPasses = 3, onPass, spec = mentions } = {}) {
  const filled = new Map();
  const passes = [];
  let remaining = posts.map((_, i) => i + 1);
  let lastErr = null;
  for (let pass = 1; pass <= maxPasses && remaining.length; pass++) {
    const idx = remaining;
    const subPosts = idx.map((n) => posts[n - 1]);
    const before = remaining.length;
    let r;
    try { r = callLabeler(subPosts); } catch (e) { lastErr = e; passes.push({ error: e.message, asked: subPosts.length }); onPass?.({ pass, maxPasses, asked: subPosts.length, got: 0, remaining: before, error: e.message }); continue; }
    passes.push({ usage: r.usage, seconds: r.seconds, tools: r.tools, asked: subPosts.length });
    for (const [localN, l] of collectLabels(r.message, subPosts.length)) filled.set(idx[localN - 1], l);
    remaining = posts.map((_, i) => i + 1).filter((n) => !filled.has(n));
    onPass?.({ pass, maxPasses, asked: subPosts.length, got: before - remaining.length, remaining: remaining.length });
  }
  if (remaining.length) throw new Error(`missing ${remaining.length} of ${posts.length} posts after ${passes.length} passes: ${remaining.slice(0, 3).join(", ")}${lastErr ? ` (last error: ${lastErr.message})` : ""}`);
  const labels = posts.map((p, i) => Object.fromEntries(spec.FIELDS.map((f) => [f, f === "id" ? p.id : filled.get(i + 1)[f]])));
  return { labels, passes };
}

// Features whose tools or prompts a labeler never needs; off, the model has no tools and the call is one turn.
const OFF = ["shell_tool", "unified_exec", "unified_exec_tty", "view_image", "sleep_tool", "tool_suggest", "multi_agent",
  "plugins", "apps", "skill_search", "memories", "goals", "image_generation", "browser_use", "computer_use", "hooks"];

// A private CODEX_HOME: the login copied from ~/.codex, no user config, AGENTS.md, plugins or hooks.
export function codexHome(dir, { model, effort, instructions, serviceTier }) {
  const home = join(dir, "home");
  mkdirSync(home, { recursive: true });
  copyFileSync(join(process.env.CODEX_HOME || join(homedir(), ".codex"), "auth.json"), join(home, "auth.json"));
  // "priority" is the Fast service tier (1.5x speed, 2x price; ~1/10 the cost of gpt-6-sol). A model
  // that does not advertise the tier silently downgrades to standard, so this is safe to default on.
  const tier = serviceTier && !["standard", "default"].includes(serviceTier) ? `service_tier = "${serviceTier}"\n` : "";
  writeFileSync(join(home, "config.toml"), `model = "${model}"\nmodel_reasoning_effort = "${effort}"\nmodel_instructions_file = "${instructions}"\nproject_doc_max_bytes = 0\n${tier}`);
  return home;
}

export function runCodex(prompt, { model = "gpt-6-luna", effort = "low", serviceTier = "priority", events, timeoutMs = 3_600_000, tries = 2, spec = mentions }) {
  const dir = mkdtempSync(join(tmpdir(), "label-codex-"));
  const schema = join(dir, "schema.json");
  const instructions = join(dir, "instructions.md");
  const last = join(dir, "last.txt");
  writeFileSync(schema, JSON.stringify(spec.SCHEMA));
  writeFileSync(instructions, spec.RULES);
  const home = codexHome(dir, { model, effort, instructions, serviceTier });
  const args = [
    "exec", "--ignore-rules", "--skip-git-repo-check", "--ephemeral", "-C", dir, "-s", "read-only",
    ...OFF.flatMap((f) => ["--disable", f]), "--output-schema", schema, "--json", "-o", last, "-",
  ];
  const t0 = Date.now();
  let r;
  for (let i = 1; i <= tries; i++) {
    r = spawnSync("codex", args, { input: prompt, encoding: "utf8", maxBuffer: 1 << 28, timeout: timeoutMs, env: { ...process.env, CODEX_HOME: home } });
    if (events) writeFileSync(events, r.stdout ?? "");
    if (r.status === 0) break;
    if (i === tries) throw new Error(`codex exec ${r.signal ? `killed by ${r.signal} after ${timeoutMs} ms` : `exited ${r.status}`}: ${(r.stderr ?? "").slice(-2000)}`);
  }
  const lines = r.stdout.split("\n").filter(Boolean).map((l) => JSON.parse(l));
  const turn = lines.find((l) => l.type === "turn.completed");
  const tools = lines.filter((l) => l.type === "item.completed" && l.item?.type !== "agent_message" && l.item?.type !== "reasoning").length;
  return { message: readFileSync(last, "utf8"), usage: turn?.usage, tools, seconds: Math.round((Date.now() - t0) / 1000) };
}

async function main() {
  const args = process.argv.slice(2);
  const input = args.find((a) => !a.startsWith("--") && !["--facts", "--vocab", "--out", "--model", "--effort", "--service-tier", "--max-passes", "--spec"].includes(args[args.indexOf(a) - 1]));
  const opt = (k, d = null) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
  const out = opt("--out");
  const facts = opt("--facts");
  const vocabPath = opt("--vocab");
  if (!input || !out || !facts || !vocabPath) {
    console.error("Usage: label-codex.mjs <chunkN.json> --facts <facts.md> --vocab <vocab.json> --out <dir> [--spec <spec.mjs>] [--model gpt-6-luna] [--effort low] [--service-tier priority] [--max-passes 3]");
    process.exit(1);
  }
  mkdirSync(out, { recursive: true });
  const n = basename(input).match(/\d+/)?.[0] ?? "0";
  const posts = JSON.parse(readFileSync(input, "utf8"));
  const events = join(out, `labels${n}.events.jsonl`);
  const vocab = existsSync(vocabPath) ? JSON.parse(readFileSync(vocabPath, "utf8")) : {};
  const appFacts = readFileSync(facts, "utf8");
  const model = opt("--model", "gpt-6-luna");
  const effort = opt("--effort", "low");
  const serviceTier = opt("--service-tier", "priority");
  const maxPasses = Math.max(1, Number(opt("--max-passes", "3")) || 3);
  const spec = await loadSpec(opt("--spec"));
  let lastRaw = "";
  const call = (subPosts) => {
    const r = runCodex(buildPrompt(appFacts, subPosts, vocab, spec), { model, effort, serviceTier, events, spec });
    lastRaw = r.message;
    return r;
  };
  // Per-pass progress to stderr so a stuck chunk is visible in real time (a short answer shows as
  // a low +got/asked); stdout keeps only the final parseable summary line.
  const onPass = ({ pass, maxPasses, asked, got, remaining, error }) =>
    process.stderr.write(`chunk ${n} pass ${pass}/${maxPasses}: ${error ? `error: ${error.slice(0, 60)}` : `+${got}/${asked}${remaining ? `, ${remaining} left` : " ✓"}`}\n`);
  const { labels, passes } = labelWithRetry(posts, call, { maxPasses, onPass, spec });
  writeFileSync(join(out, `labels${n}.raw.txt`), lastRaw);
  writeFileSync(join(out, `labels${n}.json`), JSON.stringify(labels));
  const noise = labels.filter(spec.isNoise).length;
  const about = labels.filter((l) => l.about).length;
  const seconds = passes.reduce((s, p) => s + (p.seconds || 0), 0);
  const tools = passes.reduce((s, p) => s + (p.tools || 0), 0);
  const usage = passes.reduce((a, p) => {
    for (const k of Object.keys(p.usage || {})) a[k] = (a[k] || 0) + p.usage[k];
    return a;
  }, {});
  const passNote = passes.length > 1 ? `${passes.length} passes (gap-filled ${passes.slice(1).map((p) => p.asked).join("+")}), ` : "";
  console.log(`chunk ${n}: ${labels.length} labeled, ${Math.round((100 * noise) / labels.length)}% noise, ${Math.round((100 * about) / labels.length)}% about; ${passNote}${tools} tool calls, ${seconds}s, usage ${JSON.stringify(usage)}`);
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split("/").pop())) main();
