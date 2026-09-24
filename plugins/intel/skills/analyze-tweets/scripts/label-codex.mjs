#!/usr/bin/env node
// ABOUTME: Labels one chunk with a single no-tool codex exec call in a private CODEX_HOME: the rules are the
// ABOUTME: model's instructions, app facts and posts the prompt, and the JSON answer is checked against the chunk.
//
// Usage: label-codex.mjs <chunkN.json> --facts <app-facts.md> --vocab <vocab.json> --out <dir> [--model gpt-6-luna] [--effort low]
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

// Features whose tools or prompts a labeler never needs; off, the model has no tools and the call is one turn.
const OFF = ["shell_tool", "unified_exec", "unified_exec_tty", "view_image", "sleep_tool", "tool_suggest", "multi_agent",
  "plugins", "apps", "skill_search", "memories", "goals", "image_generation", "browser_use", "computer_use", "hooks"];

// A private CODEX_HOME: the login copied from ~/.codex, no user config, AGENTS.md, plugins or hooks.
export function codexHome(dir, { model, effort, instructions }) {
  const home = join(dir, "home");
  mkdirSync(home, { recursive: true });
  copyFileSync(join(process.env.CODEX_HOME || join(homedir(), ".codex"), "auth.json"), join(home, "auth.json"));
  writeFileSync(join(home, "config.toml"), `model = "${model}"\nmodel_reasoning_effort = "${effort}"\nmodel_instructions_file = "${instructions}"\nproject_doc_max_bytes = 0\n`);
  return home;
}

export function runCodex(prompt, { model = "gpt-6-luna", effort = "low", events, timeoutMs = 3_600_000, tries = 2 }) {
  const dir = mkdtempSync(join(tmpdir(), "label-codex-"));
  const schema = join(dir, "schema.json");
  const instructions = join(dir, "instructions.md");
  const last = join(dir, "last.txt");
  writeFileSync(schema, JSON.stringify(SCHEMA));
  writeFileSync(instructions, RULES);
  const home = codexHome(dir, { model, effort, instructions });
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
  const input = args.find((a) => !a.startsWith("--") && !["--facts", "--vocab", "--out", "--model", "--effort"].includes(args[args.indexOf(a) - 1]));
  const opt = (k, d = null) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
  const out = opt("--out");
  const facts = opt("--facts");
  const vocabPath = opt("--vocab");
  if (!input || !out || !facts || !vocabPath) {
    console.error("Usage: label-codex.mjs <chunkN.json> --facts <app-facts.md> --vocab <vocab.json> --out <dir> [--model gpt-6-luna] [--effort low]");
    process.exit(1);
  }
  mkdirSync(out, { recursive: true });
  const n = basename(input).match(/\d+/)?.[0] ?? "0";
  const posts = JSON.parse(readFileSync(input, "utf8"));
  const events = join(out, `labels${n}.events.jsonl`);
  const vocab = existsSync(vocabPath) ? JSON.parse(readFileSync(vocabPath, "utf8")) : {};
  const r = runCodex(buildPrompt(readFileSync(facts, "utf8"), posts, vocab), { model: opt("--model", "gpt-6-luna"), effort: opt("--effort", "low"), events });
  writeFileSync(join(out, `labels${n}.raw.txt`), r.message);
  const labels = parseLabels(r.message, posts);
  writeFileSync(join(out, `labels${n}.json`), JSON.stringify(labels));
  const noise = labels.filter((l) => l.sentiment === "noise").length;
  const about = labels.filter((l) => l.about).length;
  console.log(`chunk ${n}: ${labels.length} labeled, ${Math.round((100 * noise) / labels.length)}% noise, ${Math.round((100 * about) / labels.length)}% about; ${r.tools} tool calls, ${r.seconds}s, usage ${JSON.stringify(r.usage)}`);
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split("/").pop())) main();
