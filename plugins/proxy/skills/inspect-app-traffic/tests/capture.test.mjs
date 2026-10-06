// ABOUTME: Tests capture.mjs: host scoping, the port check, and the hub/capture orchestration
// ABOUTME: over a faked hub (its pid is this process), so nothing launches mitmdump.
//
// The hub lifecycle and the reader scoping (since + host) are exercised live in a real
// capture; here we cover the pure logic and the orchestration that does not need a running
// mitmdump — by faking a hub whose pid is this test process, so `start`/`stop`/`status` act
// on capture records without launching anything.
import { afterEach, beforeEach, describe, test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  capFile, capturesDir, cmdCheck, cmdDown, cmdStart, cmdStatus, cmdStop, hostRegex, hubDir,
  hubMetaPath, portInUse,
} from "../scripts/capture.mjs";

function listen(port) {
  return new Promise((resolve, reject) => {
    const srv = createServer();
    srv.once("error", reject);
    srv.listen({ port, host: "0.0.0.0" }, () => resolve(srv));
  });
}

const closeServer = (srv) => new Promise((resolve) => srv.close(resolve));

describe("hostRegex", () => {
  const rx = new RegExp(hostRegex(["pump.fun", "api.pump.fun"]));

  test("matches the domain and its subdomains", () => {
    for (const host of ["pump.fun", "api.pump.fun", "frontend-api-v3.pump.fun", "pump.fun:443"]) {
      assert.match(host, rx, host);
    }
  });

  test("rejects lookalikes", () => {
    for (const host of ["notpump.fun", "pump.fund", "pump.funny.com", "evil.com"]) {
      assert.doesNotMatch(host, rx, host);
    }
  });

  test("escapes dots", () => {
    assert.doesNotMatch("pumpxfun", new RegExp(hostRegex(["pump.fun"])));
  });
});

describe("portInUse", () => {
  test("detects a wildcard listener", async () => {
    // A listener on all interfaces (0.0.0.0) must read as in-use — the case a
    // SO_REUSEADDR bind to 127.0.0.1 missed, letting a capture claim an occupied port.
    const srv = await listen(0);
    try {
      assert.equal(await portInUse(srv.address().port), true);
    } finally {
      await closeServer(srv);
    }
  });

  test("reads an unbound port as free", async () => {
    const srv = await listen(0);
    const port = srv.address().port;
    await closeServer(srv);
    assert.equal(await portInUse(port), false);
  });
});

describe("hub captures", () => {
  let tmp;
  const saved = {};

  beforeEach(() => {
    tmp = mkdtempSync(join(tmpdir(), "capture-test-"));
    for (const k of ["PROXY_DATA_DIR", "TMPDIR"]) {
      saved[k] = process.env[k];
      process.env[k] = tmp;
    }
  });

  afterEach(() => {
    for (const [k, v] of Object.entries(saved)) {
      if (v === undefined) delete process.env[k];
      else process.env[k] = v;
    }
    rmSync(tmp, { recursive: true, force: true });
  });

  function fakeHub(alive = true) {
    // A hub whose pid is this process (alive) makes ensureHub reuse it without spawning.
    mkdirSync(hubDir(), { recursive: true });
    const pid = alive ? process.pid : 2 ** 31 - 1;
    writeFileSync(hubMetaPath(), JSON.stringify(
      { pid, since: 0, port: 8080, modes: ["regular"], log: join(hubDir(), "mitmdump.log") }));
  }

  async function run(fn, args) {
    let buf = "";
    await fn(args, { out: (s) => { buf += s; }, err: () => {} });
    return JSON.parse(buf);
  }

  const startArgs = (extra) => ({ label: null, hosts: null, hostRegex: null, wireguard: false, ...extra });
  const captureRecords = () => readdirSync(capturesDir()).filter((n) => n.endsWith(".json"));

  test("start creates a capture record", async () => {
    fakeHub();
    const out = await run(cmdStart, startArgs({ label: "appA", hosts: "pump.fun,api.pump.fun" }));
    assert.equal(out.started, true);
    assert.ok(out.capture.includes("appA"));
    assert.equal(out.proxyLocal, "127.0.0.1:8080");
    const rec = JSON.parse(readFileSync(join(capturesDir(), `${out.capture}.json`), "utf8"));
    assert.deepEqual(rec.hosts, ["pump.fun", "api.pump.fun"]);
    assert.ok(rec.hostRegex.includes("pump"));
    assert.ok(rec.started_at > 0);
  });

  test("start without a hub and with the port busy fails", async (t) => {
    // No hub meta, and 8080 held by a real listener -> start reports failure, no record.
    let srv;
    try {
      srv = await listen(8080);
    } catch {
      t.skip("port 8080 not bindable in this environment");
      return;
    }
    try {
      const out = await run(cmdStart, startArgs({ label: "x" }));
      assert.equal(out.started, false);
      assert.equal(existsSync(capturesDir()) ? captureRecords().length : 0, 0);
    } finally {
      await closeServer(srv);
    }
  });

  test("stop removes the record", async () => {
    fakeHub();
    const started = await run(cmdStart, startArgs({ label: "b" }));
    const cap = started.capture;
    const out = await run(cmdStop, { capture: cap, wipe: false });
    assert.equal(out.stopped, true);
    assert.equal(existsSync(join(capturesDir(), `${cap}.json`)), false);
  });

  test("stop --wipe deletes the flow file", async () => {
    fakeHub();
    const started = await run(cmdStart, startArgs({ label: "w" }));
    const cap = started.capture;
    const f = capFile(cap);
    mkdirSync(join(f, ".."), { recursive: true });
    writeFileSync(f, "flowdata");
    const out = await run(cmdStop, { capture: cap, wipe: true });
    assert.equal(out.wiped, true);
    assert.equal(existsSync(f), false);
  });

  test("status lists the open captures", async () => {
    fakeHub();
    await run(cmdStart, startArgs({ label: "c", hosts: "x.com" }));
    const out = await run(cmdStatus, {});
    assert.equal(out.hubRunning, true);
    assert.equal(out.captures.length, 1);
  });

  test("check reports a missing capture", async () => {
    const out = await run(cmdCheck, { capture: "nope" });
    assert.equal(out.ok, false);
  });

  test("check counts from the log by since and host", async () => {
    fakeHub();
    const t0 = 1_000_000.0;
    const cap = "20260101-000000-1-x";
    mkdirSync(capturesDir(), { recursive: true });
    writeFileSync(join(capturesDir(), `${cap}.json`), JSON.stringify(
      { id: cap, label: "x", started_at: t0, hosts: ["api.foo.com"], hostRegex: hostRegex(["api.foo.com"]) }));
    writeFileSync(join(hubDir(), "mitmdump.log"),
      `PROXY_CLIENT_CONNECTED ${(t0 - 5).toFixed(3)}\n` // before start -> not counted
      + `PROXY_CLIENT_CONNECTED ${(t0 + 1).toFixed(3)}\n` // after -> counted
      + `PROXY_REQUEST ${(t0 - 1).toFixed(3)} api.foo.com\n` // before start -> no
      + `PROXY_REQUEST ${(t0 + 1).toFixed(3)} api.foo.com\n` // after + host match -> yes
      + `PROXY_REQUEST ${(t0 + 2).toFixed(3)} other.com\n`); // after but wrong host -> no
    const out = await run(cmdCheck, { capture: cap });
    assert.equal(out.ok, true);
    assert.equal(out.clientsConnected, 1);
    assert.equal(out.requests, 1);
  });

  test("check flags TLS failure", async () => {
    fakeHub();
    const t0 = 1_000_000.0;
    const cap = "20260101-000000-2-y";
    mkdirSync(capturesDir(), { recursive: true });
    writeFileSync(join(capturesDir(), `${cap}.json`), JSON.stringify(
      { id: cap, label: "y", started_at: t0, hosts: ["api.foo.com"], hostRegex: hostRegex(["api.foo.com"]) }));
    writeFileSync(join(hubDir(), "mitmdump.log"),
      `PROXY_CLIENT_CONNECTED ${(t0 + 1).toFixed(3)}\n`
      + `PROXY_TLS_FAILED ${(t0 + 1).toFixed(3)} api.foo.com\n` // CA not trusted / pinned
      + `PROXY_TLS_FAILED ${(t0 + 2).toFixed(3)} api.foo.com\n`);
    const out = await run(cmdCheck, { capture: cap });
    assert.equal(out.ok, false);
    assert.equal(out.requests, 0);
    assert.equal(out.tlsFailed, 2);
    assert.ok(out.verdict.includes("TLS"));
  });

  test("down clears a dead hub", async () => {
    fakeHub(false);
    const out = await run(cmdDown, { wipe: false });
    assert.equal(out.down, true);
    assert.equal(existsSync(hubMetaPath()), false);
  });
});
