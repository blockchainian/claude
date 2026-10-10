#!/usr/bin/env node
// ABOUTME: Downloads an audio file or URL and transcribes it locally with mlx-whisper.
// ABOUTME: Reusable audio-to-text step; any skill that has audio and needs its words.

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { loadEnvFile, outputDir, tmpDir } from "../../fetch-x-mentions/scripts/env.mjs";

export const MODEL = "mlx-community/whisper-large-v3-turbo";
export const UA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 " +
  "(KHTML, like Gecko) Chrome/126 Safari/537.36";

/** Python's SystemExit(message): printed to stderr, exit status 1. */
export class SystemExit extends Error {
  constructor(message) {
    super(message);
    this.name = "SystemExit";
  }
}

/** Python's CalledProcessError: a checked subprocess exited non-zero. */
export class CalledProcessError extends Error {
  constructor(cmd, status) {
    super(`Command '${JSON.stringify(cmd)}' returned non-zero exit status ${status}.`);
    this.name = "CalledProcessError";
    this.cmd = cmd;
    this.status = status;
  }
}

/** shutil.which: the first executable named `name` on PATH, or null. */
export function which(name) {
  for (const dir of (process.env.PATH || "").split(path.delimiter)) {
    if (!dir) continue;
    const candidate = path.join(dir, name);
    try {
      if (fs.statSync(candidate).isFile()) {
        fs.accessSync(candidate, fs.constants.X_OK);
        return candidate;
      }
    } catch {
      // not here; keep looking
    }
  }
  return null;
}

/** The command that runs mlx_whisper, installed or via uv's ephemeral env. */
export function whisperPrefix() {
  if (which("mlx_whisper")) return ["mlx_whisper"];
  if (which("uv")) return ["uv", "run", "--with", "mlx-whisper", "mlx_whisper"];
  throw new SystemExit("mlx-whisper not available: install uv, or " +
    "`pip install mlx-whisper` (Apple Silicon only)");
}

export function buildCmd(prefix, audio, outDir) {
  // Each 30 s window is decoded on its own: fed the previous window's text, one misheard line repeats until the
  // speech changes (198 times over 3 minutes of a podcast, measured 2026-10).
  return [...prefix, String(audio), "--model", MODEL, "--output-dir", String(outDir),
    "--output-format", "txt", "--verbose", "False", "--condition-on-previous-text", "False"];
}

/** Path(urlparse(src).path).suffix: the extension of the URL's path part. */
function urlSuffix(src) {
  let pathname;
  try {
    pathname = new URL(src).pathname;
  } catch {
    pathname = src; // not an absolute URL; urlparse keeps the whole thing as the path
  }
  const ext = path.posix.extname(pathname);
  return ext === "." ? "" : ext;
}

/** A local file is used in place; anything else is downloaded. */
export function getAudio(src, work) {
  try {
    if (fs.statSync(src).isFile()) return src;
  } catch {
    // not a local file
  }
  const suffix = urlSuffix(src) || ".audio";
  const dest = path.join(String(work), `download${suffix}`);
  const res = spawnSync("curl", ["-sL", "--max-time", "600", "-A", UA, "-o", dest, src],
    { encoding: "utf8" });
  let size = 0;
  try {
    size = fs.statSync(dest).size;
  } catch {
    size = 0;
  }
  if (res.status !== 0 || size === 0) {
    const stderr = (res.stderr || "").trim();
    throw new SystemExit(`download failed for ${src}: ${stderr.slice(-300)}`);
  }
  return dest;
}

/** The default transcript path: <INTEL_OUTPUT_DIR>/transcripts/<source name>.txt. */
export function transcriptPath(src, env = process.env) {
  const ext = urlSuffix(src);
  const base = path.posix.basename(/^[a-z]+:\/\//i.test(src) ? new URL(src).pathname : src);
  const name = (ext ? base.slice(0, -ext.length) : base) || "transcript";
  return path.join(outputDir(env), "transcripts", `${name}.txt`);
}

export function txtFilesIn(dir) {
  return fs.readdirSync(dir).filter((f) => f.endsWith(".txt")).sort()
    .map((f) => path.join(dir, f));
}

export function countWords(text) {
  return text.split(/\s+/).filter(Boolean).length;
}

export function main(argv = process.argv.slice(2)) {
  if (argv.length < 1) {
    throw new SystemExit("usage: transcribe-audio.mjs <audio-url-or-file> [out.txt]");
  }
  const src = argv[0];
  loadEnvFile();
  const outTxt = path.normalize(argv[1] ?? transcriptPath(src));
  const work = fs.mkdtempSync(path.join(tmpDir("transcribe"), "audio-"));
  try {
    const audio = getAudio(src, work);
    // Keep our stdout clean for the JSON; whisper's chatter goes to stderr.
    const cmd = buildCmd(whisperPrefix(), audio, work);
    const res = spawnSync(cmd[0], cmd.slice(1), { stdio: ["inherit", 2, "inherit"] });
    if (res.error) throw res.error;
    if (res.status !== 0) throw new CalledProcessError(cmd, res.status ?? res.signal);
    const produced = txtFilesIn(work);
    if (produced.length === 0) throw new SystemExit("transcription produced no .txt output");
    const text = fs.readFileSync(produced[0], "utf8");
    fs.mkdirSync(path.dirname(path.resolve(outTxt)), { recursive: true });
    fs.writeFileSync(outTxt, text, "utf8");
    const words = countWords(text);
    console.log(JSON.stringify({ transcript: outTxt, words, thin: words < 1500, source: src },
      null, 2));
  } finally {
    fs.rmSync(work, { recursive: true, force: true });
  }
}

/** Python's `if __name__ == "__main__"`: true when this module is the entry script. */
export function isMain(moduleUrl) {
  if (!process.argv[1]) return false;
  try {
    return fs.realpathSync(process.argv[1]) === fs.realpathSync(fileURLToPath(moduleUrl));
  } catch {
    return false;
  }
}

/** Run `fn` as a script: SystemExit prints its message and exits 1, like Python. */
export async function runAsScript(fn) {
  try {
    await fn();
  } catch (err) {
    if (err instanceof SystemExit) {
      console.error(err.message);
      process.exit(1);
    }
    console.error(err && err.stack ? err.stack : String(err));
    process.exit(1);
  }
}

if (isMain(import.meta.url)) {
  runAsScript(main);
}
