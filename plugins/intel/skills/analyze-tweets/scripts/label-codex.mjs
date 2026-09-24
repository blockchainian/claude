#!/usr/bin/env node
// ABOUTME: Labels one chunk with a single no-tool codex exec call in a private CODEX_HOME: the rules are the
// ABOUTME: model's instructions, app facts and posts the prompt, and the JSON answer is checked against the chunk.
//
// Usage: label-codex.mjs <chunkN.json> --facts <app-facts.md> --vocab <vocab.json> --out <dir> [--model gpt-6-luna] [--effort low] [--service-tier priority]
// Defaults to the Fast service tier ("priority": 1.5x speed, 2x price, still ~1/10 the cost of gpt-6-sol);
// pass --service-tier standard to opt out. An unsupported tier silently downgrades to standard.
// Writes labelsN.json (N from the chunk file name) plus labelsN.events.jsonl and prints one line of usage.
import { readFileSync, writeFileSync, mkdirSync, mkdtempSync, copyFileSync, existsSync } from "node:fs";
import { join, basename } from "node:path";
import { tmpdir, homedir } from "node:os";
import { spawnSync } from "node:child_process";

const FIELDS = ["id", "about", "sentiment", "topic", "feature", "point", "request", "interest"];
// The model returns a running number per post, never the 19-digit id: small models mistype long ids.
const OUT_FIELDS = ["n", ...FIELDS.slice(1)];

export const SCHEMA = {
  type: "object",
  additionalProperties: false,
  required: ["labels"],
  properties: {
    labels: {
      type: "array",
      items: {
        type: "object",
        additionalProperties: false,
        required: OUT_FIELDS,
        properties: {
          n: { type: "integer" },
          about: { type: "boolean" },
          sentiment: { type: "string", enum: ["like", "dislike", "neutral", "noise", "irrelevant"] },
          topic: { type: "string" },
          feature: { type: "string" },
          point: { type: "string" },
          request: { type: ["string", "null"] },
          interest: { type: ["string", "null"] },
        },
      },
    },
  },
};

const RULES = `
You label X posts for a reception report. Everything you need is in the message: do not run commands,
do not read or write files. Reply with the JSON only: {"labels": [...]}, one object per post, in the
order given, every post number present exactly once, fields
{n, about, sentiment, topic, feature, point, request, interest}.
Posts are one per line: n, author, likes, date, lang, text (tab separated).
Most posts are replies inside threads. A reply is about the app whenever what it says concerns the app:
its features, fees, slippage, alerts, verification, listings, outages, scams on it, its team, or a promise
it made; that holds when the reply is short, sarcastic, a question, or a rally cry. It is irrelevant only
when it talks about something else (another token, a person, a different product). It is noise only when
it says nothing (gm, emoji, "wen", address drops, giveaway begging, bot alerts, bare referral-code spam).
Sentiment is the author's attitude toward the app as the post shows it: positive is like, negative is
dislike, no attitude is neutral. Judge the attitude, not the wording or the form: a win or milestone
credited to the app, a recommendation, thanks, hype, or joy at using it are positive whatever the phrasing;
a complaint, a doubt, a demand, sarcasm, or disappointment are negative whatever the phrasing. The attitude
must be toward the app itself, not toward a token, a trade or a trader: a post that is bullish on a token,
shares a position, a thesis or an entry, or hands out a referral code, and says nothing about the app, is
neutral. A referral code next to an opinion about the app does not cancel the opinion; the stake goes in interest.
All text fields are English whatever the post's language.
`;

// The eight fields, with the vocabulary seen so far as examples (never a closed list).
export function fields(vocab = {}) {
  const list = (k) => (vocab[k] || []).join(", ") || "none yet";
  return `FIELDS (one object per post)
- about: true only if the post is about the app itself.
- sentiment: for about=true, like / dislike / neutral / noise (noise: about the app but content-free); for about=false, irrelevant (talks about something else) or noise (content-free).
- topic: what the post is about as a subject people discuss (the event, the company, the ecosystem, the culture). Examples: ${list("topics")}. If none fits, write your own. Never an "other" bucket. "none" when about=false.
- feature: which part of the app the post is about. Examples: ${list("features")}. If none fits, write your own; "none" when no part of the app applies (using the app is not a feature).
- A topic or feature you write yourself must be a 2-4 word English noun phrase, lowercase with hyphens, specific enough to tell apart from the examples, and reused for every post about the same thing.
- point: at most 12 words saying what the post claims (the bug, the number, the complaint), never the feature name alone. Empty string when there is no claim.
- request: at most 12 words when the post asks to add, fix, change or remove something, else null.
- interest: null when the speaker has no stake; otherwise the kind of stake. Examples: ${list("interests")} (referral: posts a code or link; creator-rewards: earns callout / thesis rewards; token-team: promotes their own token; official-partner: the company, staff, partners; paid-promotion). Write your own if none fits.
`;
}

export function buildPrompt(appFacts, posts, vocab = {}) {
  const lines = posts.map((t, i) => [i + 1, t.author, t.likes, t.date, t.lang, t.text.replace(/\t/g, " ")].join("\t"));
  return `${appFacts}\n${fields(vocab)}\n${posts.length} posts:\n${lines.join("\n")}\n`;
}

export function parseLabels(message, posts) {
  const labels = JSON.parse(message).labels;
  const unknown = labels.filter((l) => !Number.isInteger(l.n) || l.n < 1 || l.n > posts.length).map((l) => l.n);
  if (unknown.length) throw new Error(`${unknown.length} unknown post numbers: ${unknown.slice(0, 3).join(", ")}`);
  const got = new Set(labels.map((l) => l.n));
  const missing = posts.map((_, i) => i + 1).filter((n) => !got.has(n));
  if (missing.length) throw new Error(`missing ${missing.length} posts: ${missing.slice(0, 3).join(", ")}`);
  if (got.size !== labels.length) throw new Error(`${labels.length - got.size} duplicate post numbers`);
  labels.forEach((l, i) => { if (l.n !== i + 1) throw new Error(`order differs at ${i}: ${l.n}`); });
  return labels.map((l, i) => Object.fromEntries(FIELDS.map((f) => [f, f === "id" ? posts[i].id : l[f]])));
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
export function labelWithRetry(posts, callLabeler, { maxPasses = 3 } = {}) {
  const filled = new Map();
  const passes = [];
  let remaining = posts.map((_, i) => i + 1);
  let lastErr = null;
  for (let pass = 1; pass <= maxPasses && remaining.length; pass++) {
    const idx = remaining;
    const subPosts = idx.map((n) => posts[n - 1]);
    let r;
    try { r = callLabeler(subPosts); } catch (e) { lastErr = e; passes.push({ error: e.message, asked: subPosts.length }); continue; }
    passes.push({ usage: r.usage, seconds: r.seconds, tools: r.tools, asked: subPosts.length });
    for (const [localN, l] of collectLabels(r.message, subPosts.length)) filled.set(idx[localN - 1], l);
    remaining = posts.map((_, i) => i + 1).filter((n) => !filled.has(n));
  }
  if (remaining.length) throw new Error(`missing ${remaining.length} of ${posts.length} posts after ${passes.length} passes: ${remaining.slice(0, 3).join(", ")}${lastErr ? ` (last error: ${lastErr.message})` : ""}`);
  const labels = posts.map((p, i) => Object.fromEntries(FIELDS.map((f) => [f, f === "id" ? p.id : filled.get(i + 1)[f]])));
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

export function runCodex(prompt, { model = "gpt-6-luna", effort = "low", serviceTier = "priority", events, timeoutMs = 3_600_000, tries = 2 }) {
  const dir = mkdtempSync(join(tmpdir(), "label-codex-"));
  const schema = join(dir, "schema.json");
  const instructions = join(dir, "instructions.md");
  const last = join(dir, "last.txt");
  writeFileSync(schema, JSON.stringify(SCHEMA));
  writeFileSync(instructions, RULES);
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

function main() {
  const args = process.argv.slice(2);
  const input = args.find((a) => !a.startsWith("--") && !["--facts", "--vocab", "--out", "--model", "--effort", "--service-tier"].includes(args[args.indexOf(a) - 1]));
  const opt = (k, d = null) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
  const out = opt("--out");
  const facts = opt("--facts");
  const vocabPath = opt("--vocab");
  if (!input || !out || !facts || !vocabPath) {
    console.error("Usage: label-codex.mjs <chunkN.json> --facts <app-facts.md> --vocab <vocab.json> --out <dir> [--model gpt-6-luna] [--effort low] [--service-tier priority]");
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
  let lastRaw = "";
  const call = (subPosts) => {
    const r = runCodex(buildPrompt(appFacts, subPosts, vocab), { model, effort, serviceTier, events });
    lastRaw = r.message;
    return r;
  };
  const { labels, passes } = labelWithRetry(posts, call, { maxPasses: 3 });
  writeFileSync(join(out, `labels${n}.raw.txt`), lastRaw);
  writeFileSync(join(out, `labels${n}.json`), JSON.stringify(labels));
  const noise = labels.filter((l) => l.sentiment === "noise").length;
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
