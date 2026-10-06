// ABOUTME: Classifies which cells of a reCAPTCHA image grid hold the asked object, via a vision model
// ABOUTME: run through the local `codex exec` CLI — a one-shot call that returns structured cell indices.

import { spawn } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, copyFileSync, rmSync, existsSync } from "node:fs";
import { tmpdir, homedir } from "node:os";
import { join } from "node:path";

// Run codex exec with the prompt on stdin (the positional-prompt form hangs when stdin is a pipe).
// Resolves on a clean exit, rejects on non-zero, timeout, or spawn error. Never leaves a child behind.
function runCodex(args, prompt, env, timeoutMs) {
  return new Promise((resolve, reject) => {
    const child = spawn("codex", args, { env, stdio: ["pipe", "ignore", "ignore"] });
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      reject(new Error("codex exec timed out"));
    }, timeoutMs);
    child.on("error", (e) => {
      clearTimeout(timer);
      reject(e);
    });
    child.on("close", (code) => {
      clearTimeout(timer);
      code === 0 ? resolve() : reject(new Error(`codex exec exited ${code}`));
    });
    child.stdin.on("error", () => {});
    child.stdin.end(prompt);
  });
}

// The model is driven through `codex exec`, signed in with the user's own plan. Pinned to a strong
// multimodal model — on grainy reCAPTCHA grids it over-selects far less than the cheaper gpt-6-luna,
// which cuts the wasted rounds. Override with SECRETS_CAPTCHA_RESOLVER_MODEL if a different codex model is wanted.
const MODEL = () => process.env.SECRETS_CAPTCHA_RESOLVER_MODEL || "gpt-6-sol";
const EFFORT = () => process.env.SECRETS_CAPTCHA_RESOLVER_MODEL_EFFORT || "low";
// Service tier is a latency SLA, not a quality knob: "fast" is served quicker at the same model and
// reasoning effort, so use it for the per-round login call. Override with SECRETS_CAPTCHA_RESOLVER_MODEL_TIER.
const TIER = () => process.env.SECRETS_CAPTCHA_RESOLVER_MODEL_TIER || "fast";

// Keep only real cell indices the grid actually has (0 .. gridN*gridN-1), de-duplicated and sorted.
// The model is told the numbering, but a stray or out-of-range value must never become a tile click.
export function parseCells(text, gridN) {
  let cells;
  try {
    cells = JSON.parse(text)?.cells;
  } catch {
    return [];
  }
  if (!Array.isArray(cells)) return [];
  const max = gridN * gridN;
  const seen = new Set();
  for (const n of cells) {
    if (Number.isInteger(n) && n >= 0 && n < max) seen.add(n);
  }
  return [...seen].sort((a, b) => a - b);
}

// The prompt that frames the grid as a plain image-classification task: cell numbering, the object to
// find, and "empty if none" so the model can legitimately report that no cell matches. It is precision-
// tuned — select a cell when a part of the object genuinely extends into it (reCAPTCHA grades edge
// cells), but not for a similar-looking thing, a person, or background, and not on a guess — because
// both a wrong extra cell and a missed cell fail the challenge.
export function buildPrompt(object, gridN) {
  const n = gridN;
  const rows = [];
  for (let r = 0; r < n; r++) rows.push(`row${r}: ${Array.from({ length: n }, (_, c) => r * n + c).join(",")}`);
  return [
    `This image is a ${n}x${n} grid of ${n * n} square photo cells, numbered row-major from 0 at the top-left (${rows.join("; ")}).`,
    `Examine each cell carefully and return every cell number in which any part of ${object} is genuinely visible — include a cell even if only part of the object extends into it.`,
    `Do NOT include a cell that shows only a similar-looking object, a person, or background, and do not guess: if you are not sure a cell really contains ${object}, leave it out.`,
    `If no cell contains ${object}, return an empty list.`,
  ].join("\n");
}

// A private CODEX_HOME so the call ignores the user's own codex config/AGENTS/plugins and just pins the
// model. Reuses the existing codex login (auth.json). Returns the home dir, or null if not logged in.
function codexHome(dir) {
  const authSrc = join(process.env.CODEX_HOME || join(homedir(), ".codex"), "auth.json");
  if (!existsSync(authSrc)) return null;
  const home = join(dir, "home");
  mkdirSync(home, { recursive: true });
  copyFileSync(authSrc, join(home, "auth.json"));
  writeFileSync(
    join(home, "config.toml"),
    `model = "${MODEL()}"\nmodel_reasoning_effort = "${EFFORT()}"\n${TIER() ? `service_tier = "${TIER()}"\n` : ""}project_doc_max_bytes = 0\n`,
  );
  return home;
}

// Classify one grid image. `image` is a PNG Buffer of the whole grid; `object` is the challenge's noun
// (e.g. "a bus", "bicycles"); `gridN` is 3 or 4. Resolves to { cells } with the matching indices, or
// null on any failure (no codex login, timeout, bad output) so the caller can fall back to the human.
// Never throws.
export async function classifyImage(image, { object, gridN, timeoutMs = 60000 } = {}) {
  if (!object || !gridN) return null;
  const dir = mkdtempSync(join(tmpdir(), "grid-"));
  try {
    const home = codexHome(dir);
    if (!home) return null;
    const imgPath = join(dir, "grid.png");
    writeFileSync(imgPath, image);
    const schemaPath = join(dir, "schema.json");
    writeFileSync(
      schemaPath,
      JSON.stringify({
        type: "object",
        properties: { cells: { type: "array", items: { type: "integer" } } },
        required: ["cells"],
        additionalProperties: false,
      }),
    );
    const lastPath = join(dir, "last.txt");
    await runCodex(
      ["exec", "--skip-git-repo-check", "--ephemeral", "-C", dir, "-s", "read-only", "-i", imgPath, "--output-schema", schemaPath, "-o", lastPath, "-"],
      buildPrompt(object, gridN),
      { ...process.env, CODEX_HOME: home },
      timeoutMs,
    );
    if (!existsSync(lastPath)) return null;
    return { cells: parseCells(readFileSync(lastPath, "utf8"), gridN) };
  } catch {
    return null;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

// Parse the model's {text:"..."} answer for the password-page text CAPTCHA. Returns the transcribed
// characters with all whitespace stripped (Google's CAPTCHA string has no spaces), or "" if the output
// is malformed — the caller treats "" as "could not read" and falls back to the human.
export function parseText(out) {
  let text;
  try {
    text = JSON.parse(out)?.text;
  } catch {
    return "";
  }
  if (typeof text !== "string") return "";
  return text.replace(/\s+/g, "");
}

// The prompt for the password-page distorted-text CAPTCHA: transcribe exactly the warped characters.
export function buildTextPrompt() {
  return [
    "This image is a distorted-text CAPTCHA: one short string of letters and/or digits, warped and noisy.",
    "Return exactly the characters shown, in order, as a single string with no spaces.",
    "Preserve letter case when legible. Do not add punctuation, quotes, or any explanation.",
  ].join("\n");
}

// Read a distorted-text CAPTCHA image (a PNG Buffer) with the vision model. Resolves to { text } with
// the transcribed characters, or null on any failure (no codex login, timeout, unreadable) so the
// caller can fall back to the human. Mirrors classifyImage; never throws.
export async function readImageText(image, { timeoutMs = 60000 } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "captcha-"));
  try {
    const home = codexHome(dir);
    if (!home) return null;
    const imgPath = join(dir, "captcha.png");
    writeFileSync(imgPath, image);
    const schemaPath = join(dir, "schema.json");
    writeFileSync(
      schemaPath,
      JSON.stringify({
        type: "object",
        properties: { text: { type: "string" } },
        required: ["text"],
        additionalProperties: false,
      }),
    );
    const lastPath = join(dir, "last.txt");
    await runCodex(
      ["exec", "--skip-git-repo-check", "--ephemeral", "-C", dir, "-s", "read-only", "-i", imgPath, "--output-schema", schemaPath, "-o", lastPath, "-"],
      buildTextPrompt(),
      { ...process.env, CODEX_HOME: home },
      timeoutMs,
    );
    if (!existsSync(lastPath)) return null;
    const text = parseText(readFileSync(lastPath, "utf8"));
    return text ? { text } : null;
  } catch {
    return null;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}
