// ABOUTME: Tests that the failure capture writes its screenshot and info file in the right layout.
// ABOUTME: Uses a stub page so no browser is needed; browser behavior is verified live.

import { test, beforeEach, afterEach } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join } from "node:path";

import { spawn } from "node:child_process";

import { capture, startScreenRecording, stopScreenRecording } from "../scripts/debug.mjs";

function stubPage(url, pages = null) {
  const page = {
    url: () => url,
    context: () => ({ pages: () => pages ?? [page] }),
    screenshot: async ({ path }) => writeFileSync(path, Buffer.from([0x89, 0x50, 0x4e, 0x47])),
    evaluate: async () => ["Log in", "Continue with Google"],
    innerText: async () => "hello body text",
  };
  return page;
}

beforeEach(() => {
  process.env.SECRETS_MANAGER_STATE_PATH = mkdtempSync(join(tmpdir(), "debug-"));
});
afterEach(() => {
  delete process.env.SECRETS_MANAGER_STATE_PATH;
});

test("capture writes the screenshot and facts", async () => {
  const out = await capture(stubPage("https://accounts.google.com/x"), "a@x.com", "oauth-stuck");
  assert.ok(out);
  assert.equal(basename(dirname(out)), "a@x.com");
  assert.ok(basename(out).startsWith("oauth-stuck-"));
  assert.ok(existsSync(join(out, "screenshot.png")));
  const info = readFileSync(join(out, "info.txt"), "utf8");
  assert.ok(info.includes("accounts.google.com"));
  assert.ok(info.includes("Continue with Google"));
  assert.ok(info.includes("hello body text"));
});

test("capture records all open pages", async () => {
  const app = stubPage("https://alpha.family/");
  const popup = stubPage("https://accounts.google.com/o");
  popup.context = () => ({ pages: () => [app, popup] });
  const out = await capture(popup, "b@x.com", "oauth-timeout");
  assert.ok(readFileSync(join(out, "info.txt"), "utf8").includes("alpha.family"));
});

test("capture never throws on a dead page", async () => {
  const out = await capture({}, "c@x.com", "bad");
  assert.ok(out && existsSync(out));
  assert.ok(readFileSync(join(out, "info.txt"), "utf8").includes("unavailable"));
});

test("startScreenRecording returns null without a browser pid", () => {
  assert.equal(startScreenRecording("a@x.com", null), null);
});

test("stopScreenRecording waits for the recorder to finalize after SIGINT", async () => {
  const marker = join(process.env.SECRETS_MANAGER_STATE_PATH, "finalized");
  const script = `process.on("SIGINT", () => setTimeout(() => { require("fs").writeFileSync(${JSON.stringify(marker)}, "x"); process.exit(0); }, 400)); console.log("recording"); setInterval(() => {}, 1000);`;
  const child = spawn(process.execPath, ["-e", script], { stdio: ["ignore", "pipe", "ignore"] });
  await new Promise((r) => child.stdout.once("data", r));
  const saved = await stopScreenRecording({ child, path: "/x/rec.mov" });
  assert.equal(saved, "/x/rec.mov");
  assert.ok(existsSync(marker));
  assert.notEqual(child.exitCode, null);
});

test("stopScreenRecording returns null when the recorder never started", async () => {
  const child = spawn(process.execPath, ["-e", "console.log('skip: no app for pid')"], { stdio: ["ignore", "pipe", "ignore"] });
  await new Promise((r) => child.once("exit", r));
  assert.equal(await stopScreenRecording({ child, path: "/x/rec.mov" }), null);
});
