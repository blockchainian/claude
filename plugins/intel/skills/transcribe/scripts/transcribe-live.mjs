#!/usr/bin/env node
// ABOUTME: Transcribes a live audio stream incrementally with whisper.
// ABOUTME: ffmpeg segments the stream; each finished chunk is transcribed and appended.

import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { parseArgs } from "node:util";

import * as base from "./transcribe-audio.mjs";

const SEG_RE = /seg(\d+)\.wav$/;
const PROG = "transcribe-live.mjs";

/** The command that prints a playable URL for a platform live, or null if
 * the URL is already a direct, ffmpeg-readable stream. */
export function resolverCmd(url) {
  let host;
  try {
    host = new URL(url).host.toLowerCase();
  } catch {
    host = "";
  }
  if (host.includes("twitch.tv")) return ["streamlink", "--stream-url", url, "best"];
  if (host.includes("youtube.com") || host.includes("youtu.be")) return ["yt-dlp", "-g", url];
  if (host.includes("x.com") || host.includes("twitter.com")) return ["yt-dlp", "-g", url];
  return null;
}

export function resolveStream(url) {
  const cmd = resolverCmd(url);
  if (cmd === null) return url;
  const tool = cmd[0];
  if (base.which(tool) === null) {
    throw new base.SystemExit(`${tool} is needed to resolve ${url} — run setup.sh`);
  }
  const res = spawnSync(cmd[0], cmd.slice(1), { encoding: "utf8" });
  const stdout = (res.stdout || "").trim();
  if (res.status !== 0 || !stdout) {
    throw new base.SystemExit(`could not resolve a stream from ${url}: ` +
      `${(res.stderr || "").trim().slice(-300)}`);
  }
  return stdout.split(/\r\n|\r|\n/)[0];
}

/** ffmpeg reading the stream and writing fixed-length 16k mono wav chunks. */
export function segmentCmd(src, segDir, seconds) {
  return ["ffmpeg", "-nostdin", "-loglevel", "error", "-i", String(src),
    "-ac", "1", "-ar", "16000", "-f", "segment",
    "-segment_time", String(seconds), "-reset_timestamps", "1",
    path.join(String(segDir), "seg%05d.wav")];
}

/** Which segments are safe to transcribe. While ffmpeg runs, the highest
 * index is still being written, so only the ones below it are done; once the
 * stream has ended every segment is complete. */
export function completedIndices(indices, running) {
  const idxs = [...indices].sort((a, b) => a - b);
  if (idxs.length === 0) return [];
  return running ? idxs.slice(0, -1) : idxs;
}

export function segmentIndices(segDir) {
  const out = [];
  for (const name of fs.readdirSync(segDir)) {
    if (!name.startsWith("seg") || !name.endsWith(".wav")) continue;
    const m = SEG_RE.exec(name);
    if (m) out.push(parseInt(m[1], 10));
  }
  return out;
}

export function transcribeSegment(prefix, wav, work) {
  const outDir = path.join(String(work), "out_" + path.basename(wav, path.extname(wav)));
  fs.mkdirSync(outDir, { recursive: true });
  const cmd = base.buildCmd(prefix, wav, outDir);
  // Keep JSON on stdout, but expose the runner's diagnostics on stderr.
  const res = spawnSync(cmd[0], cmd.slice(1), { stdio: ["inherit", "ignore", "inherit"] });
  if (res.error) throw res.error;
  if (res.status !== 0) throw new base.CalledProcessError(cmd, res.status ?? res.signal);
  const produced = base.txtFilesIn(outDir);
  return produced.length ? fs.readFileSync(produced[0], "utf8") : "";
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export async function runLive(src, outTxt, seconds = 30, maxMinutes = null, onText = null) {
  if (base.which("ffmpeg") === null) throw new base.SystemExit("ffmpeg not found — run setup.sh");
  const prefix = base.whisperPrefix();
  const stream = resolveStream(src);

  const work = fs.mkdtempSync(path.join(os.tmpdir(), "tmp"));
  const segDir = path.join(work, "seg");
  fs.mkdirSync(segDir);
  outTxt = path.normalize(String(outTxt));
  fs.mkdirSync(path.dirname(path.resolve(outTxt)), { recursive: true });
  fs.writeFileSync(outTxt, "", "utf8");

  const ffCmd = segmentCmd(stream, segDir, seconds);
  const ff = spawn(ffCmd[0], ffCmd.slice(1), { stdio: ["ignore", "ignore", "ignore"] });
  let ffDone = false;
  const exited = new Promise((resolve) => {
    ff.once("exit", () => { ffDone = true; resolve(); });
    ff.once("error", () => { ffDone = true; resolve(); });
  });
  const running = () => !ffDone;

  const stop = { flag: false };
  const onSigint = () => { stop.flag = true; };
  process.on("SIGINT", onSigint);
  const deadline = maxMinutes ? Date.now() + maxMinutes * 60 * 1000 : null;

  const done = new Set();
  let chunks = 0;

  const drain = () => {
    const isRunning = running();
    for (const i of completedIndices(segmentIndices(segDir), isRunning)) {
      if (done.has(i)) continue;
      const wav = path.join(segDir, `seg${String(i).padStart(5, "0")}.wav`);
      const text = transcribeSegment(prefix, wav, work).trim();
      done.add(i);
      chunks += 1;
      if (text) {
        fs.appendFileSync(outTxt, text + "\n", "utf8");
        if (onText) onText(text);
      }
    }
  };

  try {
    while (running()) {
      if (stop.flag || (deadline && Date.now() > deadline)) {
        ff.kill("SIGINT"); // let ffmpeg finalize the open chunk
        break;
      }
      drain();
      await sleep(1000);
    }
    const timedOut = await Promise.race([exited.then(() => false), sleep(30000).then(() => true)]);
    if (timedOut) ff.kill("SIGTERM");
    drain(); // final pass: the stream ended, so the last chunk is now complete
  } finally {
    process.off("SIGINT", onSigint);
    if (running()) ff.kill("SIGTERM");
  }

  const textAll = fs.readFileSync(outTxt, "utf8");
  const words = base.countWords(textAll);
  return { transcript: outTxt, words, thin: words < 1500, source: src, chunks };
}

const USAGE = `usage: ${PROG} [-h] [--segment-seconds SEGMENT_SECONDS] ` +
  `[--max-minutes MAX_MINUTES]\n` +
  `${" ".repeat(7 + PROG.length)}source out_txt`;

const HELP = `${USAGE}

Transcribe a live audio stream.

positional arguments:
  source                stream URL (direct or Twitch/YouTube/X) or file
  out_txt               transcript file, appended as chunks land

options:
  -h, --help            show this help message and exit
  --segment-seconds SEGMENT_SECONDS
                        chunk length; whisper's window is 30s (default)
  --max-minutes MAX_MINUTES
                        stop after this long (default: until the stream ends)
`;

/** argparse's behaviour: usage + error on stderr, exit status 2. */
function usageError(message) {
  process.stderr.write(`${USAGE}\n${PROG}: error: ${message}\n`);
  process.exit(2);
}

export function parseCli(argv) {
  let parsed;
  try {
    parsed = parseArgs({
      args: argv,
      allowPositionals: true,
      options: {
        help: { type: "boolean", short: "h" },
        "segment-seconds": { type: "string" },
        "max-minutes": { type: "string" },
      },
    });
  } catch (err) {
    usageError(err.message);
  }
  const { values, positionals } = parsed;
  if (values.help) {
    process.stdout.write(HELP);
    process.exit(0);
  }
  const missing = ["source", "out_txt"].slice(positionals.length);
  if (missing.length) usageError(`the following arguments are required: ${missing.join(", ")}`);
  if (positionals.length > 2) {
    usageError(`unrecognized arguments: ${positionals.slice(2).join(" ")}`);
  }
  let segmentSeconds = 30;
  if (values["segment-seconds"] !== undefined) {
    const raw = values["segment-seconds"];
    if (!/^[+-]?\d+$/.test(raw.trim())) {
      usageError(`argument --segment-seconds: invalid int value: '${raw}'`);
    }
    segmentSeconds = parseInt(raw, 10);
  }
  let maxMinutes = null;
  if (values["max-minutes"] !== undefined) {
    const raw = values["max-minutes"];
    maxMinutes = Number(raw.trim());
    if (raw.trim() === "" || Number.isNaN(maxMinutes)) {
      usageError(`argument --max-minutes: invalid float value: '${raw}'`);
    }
  }
  return { source: positionals[0], outTxt: positionals[1], segmentSeconds, maxMinutes };
}

export async function main(argv = process.argv.slice(2)) {
  const a = parseCli(argv);
  const summary = await runLive(a.source, a.outTxt, a.segmentSeconds, a.maxMinutes,
    (t) => process.stderr.write(t + "\n"));
  console.log(JSON.stringify(summary, null, 2));
}

if (base.isMain(import.meta.url)) {
  base.runAsScript(main);
}
