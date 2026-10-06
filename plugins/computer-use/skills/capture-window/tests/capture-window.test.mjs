// ABOUTME: Tests capture-window's argument and missing-window errors by running the real script.
// ABOUTME: Window captures themselves need a live desktop and are checked by hand.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import test from "node:test";

const script = new URL("../scripts/capture-window", import.meta.url).pathname;

test("prints usage when the app name is missing", () => {
  const run = spawnSync(script, [], { encoding: "utf8" });
  assert.notEqual(run.status, 0);
  assert.match(run.stderr, /usage: capture-window <app name> <out.png>/);
});

test("fails with a clear message when the app has no on-screen window", () => {
  const run = spawnSync(script, ["no-such-app-anywhere", "/dev/null"], { encoding: "utf8" });
  assert.equal(run.status, 1);
  assert.match(run.stderr, /no on-screen window for 'no-such-app-anywhere'/);
});
