// ABOUTME: Tests that the stored window position is written into a profile's xulstore.json correctly.
// ABOUTME: Pure file handling; where the browser actually opens is verified live.

import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { EventEmitter } from "node:events";

import { displayName, movePopupsToDisplay, storeWindowPosition } from "../scripts/window-place.mjs";

const DOC = "chrome://browser/content/browser.xhtml";
const read = (dir) => JSON.parse(readFileSync(join(dir, "xulstore.json"), "utf8"));

test("storeWindowPosition creates xulstore.json for a fresh profile", () => {
  const dir = mkdtempSync(join(tmpdir(), "wp-"));
  storeWindowPosition(dir, -1472, 887);
  assert.deepEqual(read(dir), { [DOC]: { "main-window": { screenX: "-1472", screenY: "887" } } });
});

test("storeWindowPosition keeps the other stored state of an existing profile", () => {
  const dir = mkdtempSync(join(tmpdir(), "wp-"));
  const existing = {
    [DOC]: { "main-window": { screenX: "4", screenY: "31", width: "1728" }, "sidebar-box": { checked: "true" } },
    "chrome://other/x.xhtml": { a: { b: "c" } },
  };
  writeFileSync(join(dir, "xulstore.json"), JSON.stringify(existing));
  storeWindowPosition(dir, -1472, 887);
  const after = read(dir);
  assert.deepEqual(after[DOC]["main-window"], { screenX: "-1472", screenY: "887", width: "1728" });
  assert.deepEqual(after[DOC]["sidebar-box"], { checked: "true" });
  assert.deepEqual(after["chrome://other/x.xhtml"], { a: { b: "c" } });
});

test("storeWindowPosition replaces an unreadable xulstore.json", () => {
  const dir = mkdtempSync(join(tmpdir(), "wp-"));
  writeFileSync(join(dir, "xulstore.json"), "{not json");
  storeWindowPosition(dir, 10, 20);
  assert.deepEqual(read(dir), { [DOC]: { "main-window": { screenX: "10", screenY: "20" } } });
});

test("displayName is BROWSER_DISPLAY, or empty for the main display", () => {
  assert.equal(displayName({ BROWSER_DISPLAY: "SAMSUNG" }), "SAMSUNG");
  assert.equal(displayName({}), "");
});

test("movePopupsToDisplay moves the profile's windows each time a new page opens", () => {
  const context = new EventEmitter();
  const moved = [];
  movePopupsToDisplay(context, "/profiles/a", (dir) => moved.push(dir));
  assert.deepEqual(moved, []);
  context.emit("page", {});
  context.emit("page", {});
  assert.deepEqual(moved, ["/profiles/a", "/profiles/a"]);
});
