// ABOUTME: Regression tests for claim-simulator.mjs and capture-slice.sh: one agent per simulator
// ABOUTME: or phone, refusals name the holder, and capture needs the claim and never "booted".

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptsDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "scripts");
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "claim-simulator-test-"));
const UDID = "00000000-0000-0000-0000-00000000TEST";
const PHONE = "00008030-001A2B3C4D5E6F7A";

function run(script, ...args) {
  const res = spawnSync(path.join(scriptsDir, script), args, {
    encoding: "utf8",
    env: { ...process.env, TMPDIR: tmp },
  });
  return { status: res.status, stdout: res.stdout || "", stderr: res.stderr || "" };
}

function claim(...args) {
  return run("claim-simulator.mjs", ...args);
}

function capture(udid, runId) {
  return run("capture-slice.sh", "--simulator", udid, "--run", runId, "--out", `${tmp}/x.png`);
}

function parse(stdout) {
  return stdout ? JSON.parse(stdout) : {};
}

describe("one run holds a simulator or phone at a time, and capture needs the claim", () => {
  test("first claim succeeds and writes the lock file", () => {
    const r = claim(UDID, "--run", "run-a");
    assert.equal(r.status, 0, r.stderr);
    const j = parse(r.stdout);
    assert.equal(j.claimed, true);
    assert.equal(j.heldBy, "run-a");
    assert.equal(j.stolenFrom, null);
    assert.equal(j.lock, path.join(tmp, `ios-screenshot-lock.${UDID}.json`));
    const lock = JSON.parse(fs.readFileSync(j.lock, "utf8"));
    assert.equal(lock.run, "run-a");
    assert.equal(typeof lock.since, "number");
    assert.equal(typeof lock.pid, "number");
  });

  test("re-claiming by the same run is idempotent", () => {
    const r = claim(UDID, "--run", "run-a");
    assert.equal(r.status, 0, r.stderr);
    assert.equal(parse(r.stdout).claimed, true);
  });

  test("a second run's claim is refused as held, naming the holder and what to do", () => {
    const r = claim(UDID, "--run", "run-b");
    const j = parse(r.stdout);
    assert.equal(r.status, 3, r.stdout);
    assert.equal(j.claimed, false);
    assert.equal(j.heldBy, "run-a");
    assert.equal(typeof j.ageSeconds, "number");
    assert.ok(r.stderr.includes("run-a"), r.stderr);
    assert.ok(r.stderr.toLowerCase().includes("wait"), r.stderr);
  });

  test("another run cannot release the lock", () => {
    const r = claim(UDID, "--run", "run-b", "--release");
    assert.equal(r.status, 3);
    assert.deepEqual(parse(r.stdout), { released: false, udid: UDID, heldBy: "run-a" });
  });

  test('"booted" is not accepted as a UDID', () => {
    const r = claim("booted", "--run", "run-a");
    assert.equal(r.status, 2);
  });

  test('capture refuses "booted"', () => {
    const r = capture("booted", "run-a");
    assert.equal(r.status, 2, r.stderr);
  });

  test("capture by a run that does not hold the claim is refused", () => {
    const r = capture(UDID, "run-b");
    assert.equal(r.status, 3, r.stderr);
    assert.ok(r.stderr.includes("run-a"), r.stderr);
  });

  test("a held phone is refused with its holder and is not called a simulator", () => {
    claim(PHONE, "--run", "run-a");
    const r = claim(PHONE, "--run", "run-b");
    assert.equal(r.status, 3);
    assert.ok(!r.stderr.toLowerCase().includes("simulator"), r.stderr);
    assert.ok(r.stderr.includes("run-a"), r.stderr);
  });

  test("a phone release refusal does not call it a simulator", () => {
    const r = claim(PHONE, "--run", "run-b", "--release");
    assert.equal(r.status, 3);
    assert.ok(!r.stderr.toLowerCase().includes("simulator"), r.stderr);
  });

  test("--steal takes the lock", () => {
    const r = claim(UDID, "--run", "run-b", "--steal");
    assert.equal(r.status, 0, r.stderr);
    const j = parse(r.stdout);
    assert.equal(j.heldBy, "run-b");
    assert.equal(j.stolenFrom, "run-a");
  });

  test("the holder can release", () => {
    const r = claim(UDID, "--run", "run-b", "--release");
    assert.equal(r.status, 0, r.stderr);
    assert.deepEqual(parse(r.stdout), { released: true, udid: UDID });
    assert.ok(!fs.existsSync(path.join(tmp, `ios-screenshot-lock.${UDID}.json`)));
  });

  test("capture does not run against an unclaimed simulator", () => {
    const r = capture(UDID, "run-b");
    assert.equal(r.status, 3, r.stderr);
    assert.ok(r.stderr.includes("claim"), r.stderr);
  });

  test("releasing an unheld lock is a no-op", () => {
    const r = claim(UDID, "--run", "run-b", "--release");
    assert.equal(r.status, 0);
  });

  test("a missing --run is a usage error", () => {
    const r = claim(UDID);
    assert.equal(r.status, 2);
    assert.ok(r.stderr.includes("--run"), r.stderr);
  });
});
