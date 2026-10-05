// ABOUTME: Dumps a screenshot and page facts when a login step fails, and screen-records headed runs,
// ABOUTME: for offline inspection. Best-effort and never throws, so a capture failure can't mask errors.

import { mkdirSync, writeFileSync } from "node:fs";
import { spawn } from "node:child_process";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { debugDir } from "./config.mjs";

const RECORD_SCRIPT = fileURLToPath(new URL("./recordWindows.swift", import.meta.url));

function stamp() {
  const d = new Date();
  const p = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`;
}

// Save a raw artifact (e.g. the exact grid PNG sent to the classifier) under debugDir()/<email>/, named
// so the challenge and the model's answer can be checked by eye afterward. Best-effort; returns the path
// or null. This is how a headed run accumulates a labeled sample set for measuring classifier accuracy.
export function write(email, name, buffer) {
  try {
    const dir = join(debugDir(), email);
    mkdirSync(dir, { recursive: true });
    const path = join(dir, `${stamp()}-${name}`);
    writeFileSync(path, buffer);
    return path;
  } catch {
    return null;
  }
}

// Start recording the headed browser's windows (the process `pid`, so an OAuth popup is included and
// other accounts' windows are not) to debugDir/<email>/rec-<ts>.mov with the cursor shown. A headed run
// is a debug run: the recording is what lets the reCAPTCHA grids be reviewed frame-by-frame and
// re-tested against a classifier afterward. Best-effort: returns a handle for stopScreenRecording, or
// null (non-macOS, no browser pid, or spawn failed).
export function startScreenRecording(email, pid) {
  if (process.platform !== "darwin" || !pid) return null;
  try {
    const dir = join(debugDir(), email);
    mkdirSync(dir, { recursive: true });
    const path = join(dir, `rec-${stamp()}.mov`);
    const child = spawn("swift", [RECORD_SCRIPT, String(pid), path], { stdio: "ignore" });
    child.on("error", () => {});
    return { child, path };
  } catch {
    return null;
  }
}

// Stop a recording started by startScreenRecording. The recorder finalizes the .mov on SIGINT and then
// exits, so this waits for that exit (a kill before it leaves an unplayable file). Returns the saved
// path, or null when the recorder had already exited without recording. Best-effort; never throws.
export async function stopScreenRecording(handle, timeoutMs = 15000) {
  const child = handle?.child;
  if (!child || child.exitCode !== null || child.signalCode !== null) return null;
  let timer;
  try {
    const exited = new Promise((r) => child.once("exit", r));
    child.kill("SIGINT");
    // The timeout is cleared once the recorder exits: left armed, it would hold the process open.
    await Promise.race([exited, new Promise((r) => (timer = setTimeout(r, timeoutMs)))]);
    return handle.path;
  } catch {
    return null;
  } finally {
    clearTimeout(timer);
  }
}

// Write a screenshot and page facts for one failed step; return its directory. Lands under
// debugDir()/<email>/<label>-<ts>/. Every field is read independently and a missing one is noted
// rather than aborting the dump, so a half-dead page still yields whatever it can. Never throws.
export async function capture(surface, email, label) {
  let out;
  try {
    out = join(debugDir(), email, `${label}-${stamp()}`);
    mkdirSync(out, { recursive: true });
  } catch {
    return null;
  }
  try {
    await surface.screenshot({ path: join(out, "screenshot.png") });
  } catch {
    /* no screenshot */
  }
  const fields = [
    ["url", async () => surface.url()],
    ["pages", async () => surface.context().pages().map((p) => p.url())],
    [
      "buttons",
      async () =>
        surface.evaluate(() =>
          [...document.querySelectorAll("button,a,[role=button]")]
            .map((e) => (e.innerText || "").trim())
            .filter(Boolean)
            .slice(0, 60),
        ),
    ],
    ["body", async () => (await surface.innerText("body")).slice(0, 3000)],
  ];
  const lines = [];
  for (const [name, read] of fields) {
    try {
      lines.push(`== ${name} ==\n${JSON.stringify(await read())}`);
    } catch (e) {
      lines.push(`== ${name} == (unavailable: ${e?.message ?? e})`);
    }
  }
  try {
    writeFileSync(join(out, "info.txt"), lines.join("\n\n"));
  } catch {
    /* nothing more to record */
  }
  return out;
}
