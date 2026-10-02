// ABOUTME: Puts an account's Camoufox windows on the built-in display, off the main screen: the main
// ABOUTME: window opens there, popups are moved there. Best-effort and macOS-only; any failure is a no-op.

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

// System Events cannot see the Camoufox process (it is not a registered GUI app), and JXA cannot
// build the CGPoint an AXValue needs, so the raw Accessibility calls live in a Swift script.
const MOVE_SCRIPT = fileURLToPath(new URL("./moveWindows.swift", import.meta.url));
const DISPLAY_SCRIPT = fileURLToPath(new URL("./builtinDisplay.swift", import.meta.url));
const BROWSER_DOC = "chrome://browser/content/browser.xhtml";

// The main camoufox process for this profile (not the gpu/plugin child processes).
export function findPid(profileDir) {
  const out = execFileSync("ps", ["-Ao", "pid=,args="], { encoding: "utf8" });
  for (const line of out.split("\n")) {
    if (
      line.includes(profileDir) &&
      line.includes("MacOS/camoufox") &&
      !line.includes("gpu-helper") &&
      !line.includes("plugin-container")
    ) {
      const pid = Number.parseInt(line.trim().split(/\s+/, 1)[0], 10);
      if (Number.isInteger(pid)) return pid;
    }
  }
  return null;
}

// Record (x, y) as the browser window's position in the profile's xulstore.json, where Firefox reads
// it at launch. Everything else stored there is kept.
export function storeWindowPosition(profileDir, x, y) {
  const path = join(profileDir, "xulstore.json");
  let stored = {};
  try {
    stored = JSON.parse(readFileSync(path, "utf8"));
  } catch {
    /* fresh profile, or an unreadable file that Firefox would discard too */
  }
  const doc = (stored[BROWSER_DOC] ??= {});
  doc["main-window"] = { ...doc["main-window"], screenX: String(x), screenY: String(y) };
  writeFileSync(path, JSON.stringify(stored));
}

// Make this profile's next launch open its window on the built-in display. A no-op (returns false)
// off macOS and when there is no built-in display. Must run before the browser launches.
export function openOnBuiltin(profileDir, margin = 40) {
  if (process.platform !== "darwin") return false;
  try {
    const out = execFileSync("swift", [DISPLAY_SCRIPT], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
      timeout: 60000,
    });
    const [x, y] = out.trim().split(" ").map(Number);
    if (!Number.isInteger(x) || !Number.isInteger(y)) return false;
    storeWindowPosition(profileDir, x + margin, y + margin);
    return true;
  } catch {
    return false;
  }
}

// Move every window of this account's Camoufox that is off the built-in display onto it. Needed for
// OAuth popups: the app positions them itself, from screen coordinates that land on the main display.
// A no-op (returns false) off macOS, when there is no built-in display, when the built-in already is
// the main display, or when Accessibility trust is missing. Returns true once a window has been placed.
export function moveToBuiltin(profileDir, margin = 40) {
  if (process.platform !== "darwin") return false;
  try {
    const pid = findPid(profileDir);
    if (!pid) return false;
    const out = execFileSync("swift", [MOVE_SCRIPT, String(pid), String(margin)], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
      timeout: 60000,
    });
    return out.trim() === "moved";
  } catch {
    return false;
  }
}

// Move this profile's windows to the built-in display whenever the browser opens a new page, so a
// popup leaves the main screen as soon as it appears.
export function movePopupsToBuiltin(context, profileDir, move = moveToBuiltin) {
  context.on("page", () => move(profileDir));
}
