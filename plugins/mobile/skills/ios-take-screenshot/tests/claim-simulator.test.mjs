// ABOUTME: Regression tests for claim-simulator.mjs and capture-slice.sh: one agent per simulator
// ABOUTME: or phone, refusals name the holder, and capture needs the claim and never "booted".

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
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


describe("concurrent claims and existing state", () => {
  function concurrentClaim(udid, runId) {
    return new Promise((resolve, reject) => {
      const child = spawn(path.join(scriptsDir, "claim-simulator.mjs"), [udid, "--run", runId], {
        env: { ...process.env, TMPDIR: tmp },
      });
      let stdout = "", stderr = "";
      child.stdout.on("data", (data) => { stdout += data; });
      child.stderr.on("data", (data) => { stderr += data; });
      child.on("error", reject);
      child.on("close", (status) => resolve({ status, stdout, stderr }));
    });
  }

  test("simultaneous callers get exactly one holder per device", async () => {
    const devices = ["CONCURRENT-A", "CONCURRENT-B"];
    const results = await Promise.all(devices.flatMap((udid) =>
      Array.from({ length: 8 }, (_, i) => concurrentClaim(udid, `${udid}-${i}`))));
    for (const udid of devices) {
      const group = results.filter((r) => parse(r.stdout).udid === udid);
      assert.equal(group.length, 8);
      const winners = group.filter((r) => r.status === 0);
      assert.equal(winners.length, 1, JSON.stringify(group));
      const holder = parse(winners[0].stdout).heldBy;
      for (const result of group.filter((r) => r.status !== 0)) {
        assert.equal(result.status, 3, result.stderr);
        assert.equal(parse(result.stdout).heldBy, holder);
      }
      assert.equal(JSON.parse(fs.readFileSync(path.join(tmp, `ios-screenshot-lock.${udid}.json`))).run, holder);
      assert.equal(claim(udid, "--run", holder, "--release").status, 0);
    }
  });

  test("reclaim preserves an active phone session and original claim age", () => {
    const file = path.join(tmp, "ios-screenshot-lock.RECLAIM-PHONE.json");
    const held = { run: "run-a", since: 123, session: "live-session", sessionOwner: "caller-a" };
    fs.writeFileSync(file, JSON.stringify(held));
    assert.equal(claim("RECLAIM-PHONE", "--run", "run-a").status, 0);
    assert.deepEqual(JSON.parse(fs.readFileSync(file)), held);
    assert.equal(claim("RECLAIM-PHONE", "--run", "run-a", "--release").status, 3);
    assert.deepEqual(JSON.parse(fs.readFileSync(file)), held);
  });

  test("explicit steal recovers an interrupted create", () => {
    const file = path.join(tmp, "ios-screenshot-lock.DEAD-CREATE.json");
    fs.writeFileSync(file, JSON.stringify({ run: "dead", pending: { caller: "old", call: "old-call" } }));
    assert.equal(claim("DEAD-CREATE", "--run", "new", "--steal").status, 0);
    const held = JSON.parse(fs.readFileSync(file));
    assert.equal(held.run, "new");
    assert.equal(held.pending, undefined);
  });

  test("a corrupt claim fails explicitly and is never replaced", () => {
    const file = path.join(tmp, "ios-screenshot-lock.CORRUPT.json");
    fs.writeFileSync(file, "{unfinished");
    const result = claim("CORRUPT", "--run", "run-a");
    assert.notEqual(result.status, 0);
    assert.equal(fs.readFileSync(file, "utf8"), "{unfinished");
  });

  test("device identifiers cannot escape the shared claim directory", () => {
    assert.equal(claim("../other", "--run", "run-a").status, 2);
    assert.equal(claim("VALID", "--run", "").status, 2);
  });
});


test("claims work from a relocated plugin path containing spaces", () => {
  const plugin = path.join(tmp, "mobile plugin with spaces");
  const scripts = path.join(plugin, "skills", "ios-take-screenshot", "scripts");
  fs.mkdirSync(scripts, { recursive: true });
  fs.mkdirSync(path.join(plugin, "hooks"));
  fs.copyFileSync(path.join(scriptsDir, "claim-simulator.mjs"), path.join(scripts, "claim-simulator.mjs"));
  fs.copyFileSync(path.resolve(scriptsDir, "../../../hooks/claim_state.py"), path.join(plugin, "hooks", "claim_state.py"));
  const result = spawnSync(process.execPath, [path.join(scripts, "claim-simulator.mjs"), "RELOCATED", "--run", "run-a"], {
    cwd: os.tmpdir(), encoding: "utf8", env: { ...process.env, TMPDIR: tmp },
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(parse(result.stdout).claimed, true);
});
