// ABOUTME: Tests batch + live transcription command shapes and, if tools exist, e2e.
// ABOUTME: The e2e paths generate a real clip with ffmpeg and run whisper on it.

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import * as audio from "../scripts/transcribe-audio.mjs";
import * as live from "../scripts/transcribe-live.mjs";

const scriptsDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "scripts");
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "transcribe-test-"));

function makeClip(file, seconds) {
  const res = spawnSync("ffmpeg", ["-v", "error", "-f", "lavfi", "-i",
    `sine=frequency=440:duration=${seconds}`, "-ac", "1", "-ar", "16000", file]);
  assert.equal(res.status, 0, "ffmpeg could not generate a clip");
}

describe("batch: buildCmd shape", () => {
  const cmd = audio.buildCmd(["mlx_whisper"], "/tmp/a.mp3", "/tmp/out");

  test("has the model", () => {
    assert.ok(cmd.includes("mlx-community/whisper-large-v3-turbo"));
  });

  test("asks for txt output", () => {
    assert.ok(cmd.includes("--output-format") && cmd.includes("txt"));
  });

  test("points at the audio and the out dir", () => {
    assert.ok(cmd.includes("/tmp/a.mp3"));
    assert.ok(cmd.includes("/tmp/out"));
  });

  test("does not feed one window's text to the next, which loops a misheard line for minutes", () => {
    const at = cmd.indexOf("--condition-on-previous-text");
    assert.ok(at > 0 && cmd[at + 1] === "False");
  });
});

describe("batch: getAudio", () => {
  test("returns a local file unchanged (no download)", () => {
    const existing = path.join(tmp, "already.mp3");
    fs.writeFileSync(existing, "not really audio");
    assert.equal(audio.getAudio(existing, tmp), existing);
  });
});

describe("live: completedIndices (the chunk-readiness core)", () => {
  test("no segments -> none complete", () => {
    assert.deepEqual(live.completedIndices([], true), []);
  });

  test("one segment, still running -> none complete (still being written)", () => {
    assert.deepEqual(live.completedIndices([0], true), []);
  });

  test("running -> all but the last are complete", () => {
    assert.deepEqual(live.completedIndices([0, 1, 2], true), [0, 1]);
  });

  test("stream ended -> every segment is complete", () => {
    assert.deepEqual(live.completedIndices([0, 1, 2], false), [0, 1, 2]);
  });

  test("sorts its input", () => {
    assert.deepEqual(live.completedIndices([2, 0, 1], true), [0, 1]);
  });
});

describe("live: segmentCmd shape", () => {
  const cmd = live.segmentCmd("http://x/live.m3u8", tmp, 30);

  test("segments", () => {
    assert.ok(cmd.includes("segment") && cmd.includes("-segment_time"));
  });

  test("uses 30s chunks", () => {
    assert.ok(cmd.includes("30"));
  });

  test("writes 16k mono wav", () => {
    assert.ok(cmd.includes("16000"));
    assert.ok(cmd[cmd.length - 1].endsWith(".wav"));
  });
});

describe("live: platform resolution chooser (no network)", () => {
  test("twitch -> streamlink", () => {
    assert.equal(live.resolverCmd("https://www.twitch.tv/foo")[0], "streamlink");
  });

  test("youtube -> yt-dlp", () => {
    assert.equal(live.resolverCmd("https://youtube.com/watch?v=x")[0], "yt-dlp");
  });

  test("youtu.be -> yt-dlp", () => {
    assert.equal(live.resolverCmd("https://youtu.be/x")[0], "yt-dlp");
  });

  test("x spaces -> yt-dlp", () => {
    assert.equal(live.resolverCmd("https://x.com/i/spaces/1")[0], "yt-dlp");
  });

  test("direct m3u8 -> no resolver", () => {
    assert.equal(live.resolverCmd("https://cdn/live.m3u8"), null);
  });

  test("direct mp3 stream -> no resolver", () => {
    assert.equal(live.resolverCmd("https://cdn/audio.mp3"), null);
  });
});

describe("setup.sh --check is a no-op dry run", () => {
  const res = spawnSync("bash", [path.join(scriptsDir, "setup.sh"), "--check"],
    { encoding: "utf8" });

  test("exits 0", () => {
    assert.equal(res.status, 0, res.stderr.slice(-300));
  });

  test("reports ffmpeg", () => {
    assert.ok((res.stdout + res.stderr).includes("ffmpeg"), res.stdout.slice(-200));
  });
});

// e2e only if ffmpeg + a whisper runner exist. mlx-whisper itself only installs
// on Apple Silicon, so a `uv` on a Linux box is not a usable runner.
const haveFfmpeg = Boolean(audio.which("ffmpeg"));
const haveWhisper = Boolean(audio.which("mlx_whisper") || audio.which("uv"));
const appleSilicon = process.platform === "darwin" && process.arch === "arm64";
const e2eSkip = !haveFfmpeg || !haveWhisper
  ? "ffmpeg or whisper runner missing"
  : !appleSilicon ? "mlx-whisper needs Apple Silicon" : false;

describe("e2e whisper smoke", { skip: e2eSkip }, () => {
  test("batch transcribes a generated clip and prints JSON", () => {
    const clip = path.join(tmp, "clip.wav");
    makeClip(clip, 1);
    const out = path.join(tmp, "t.txt");
    const res = spawnSync(process.execPath, [path.join(scriptsDir, "transcribe-audio.mjs"),
      clip, out], { encoding: "utf8", timeout: 120_000 });
    assert.ifError(res.error);
    assert.equal(res.status, 0, res.stderr);
    assert.ok(fs.statSync(out).isFile(), "wrote out.txt");
    const js = JSON.parse(res.stdout);
    assert.equal(js.transcript, out, res.stdout.slice(-200));
  });

  // live: a finite local file stands in for a stream — ffmpeg segments it,
  // the loop drains every chunk once ffmpeg exits, and writes JSON.
  test("live segments a generated clip and prints a JSON summary", () => {
    const longClip = path.join(tmp, "long.wav");
    makeClip(longClip, 5);
    const out = path.join(tmp, "live.txt");
    const res = spawnSync(process.execPath, [path.join(scriptsDir, "transcribe-live.mjs"),
      longClip, out, "--segment-seconds", "2"], { encoding: "utf8", timeout: 120_000 });
    assert.ifError(res.error);
    assert.equal(res.status, 0, res.stderr);
    assert.ok(fs.statSync(out).isFile(), "wrote out.txt");
    const js = JSON.parse(res.stdout);
    assert.equal(js.transcript, out, res.stdout.slice(-200));
    assert.equal(js.chunks, 3, res.stdout);
  });
});
