// ABOUTME: Puts an account's Camoufox windows on one display, SECRETS_BROWSER_DISPLAY (else the main one): the main
// ABOUTME: window opens there, popups are moved there. Best-effort and macOS-only; any failure is a no-op.

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

// System Events cannot see the Camoufox process (it is not a registered GUI app), and JXA cannot
// build the CGPoint an AXValue needs, so the raw Accessibility calls live in a Swift script.
const MOVE_SCRIPT = fileURLToPath(new URL("./moveWindows.swift", import.meta.url));
const DISPLAY_SCRIPT = fileURLToPath(new URL("./displayOrigin.swift", import.meta.url));
const BROWSER_DOC = "chrome://browser/content/browser.xhtml";

// The display the windows go on: SECRETS_BROWSER_DISPLAY in the secrets-manager's .env, any part of the
// display's name in any case (e.g. SAMSUNG); empty means the main display.
export const displayName = (env = process.env) => env.SECRETS_BROWSER_DISPLAY ?? "";

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

// Make this profile's next launch open its window on the display. A no-op (returns false) off macOS
// and when no connected display has that name. Must run before the browser launches.
export function openOnDisplay(profileDir, name = displayName(), margin = 40) {
  if (process.platform !== "darwin") return false;
  try {
    const out = execFileSync("swift", [DISPLAY_SCRIPT, name], {
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

// Move every window of this account's Camoufox that is off the display onto it. Needed for OAuth
// popups: the app positions them itself, from screen coordinates of its own choosing. A no-op
// (returns false) off macOS, when no connected display has that name, or when Accessibility trust is
// missing. Returns true once a window has been placed.
export function moveToDisplay(profileDir, name = displayName(), margin = 40) {
  if (process.platform !== "darwin") return false;
  try {
    const pid = findPid(profileDir);
    if (!pid) return false;
    const out = execFileSync("swift", [MOVE_SCRIPT, String(pid), String(margin), name], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
      timeout: 60000,
    });
    return out.trim() === "moved";
  } catch {
    return false;
  }
}

// Move this profile's windows to the display whenever the browser opens a new page, so a popup
// joins the others as soon as it appears.
export function movePopupsToDisplay(context, profileDir, move = moveToDisplay) {
  context.on("page", () => move(profileDir));
}
