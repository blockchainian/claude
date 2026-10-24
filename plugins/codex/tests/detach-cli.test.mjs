// ABOUTME: Exercises detach from another process, including live daemon status reconciliation.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { SessionStore, writeSupervisor } from "../lib/inbox.mjs";
import { WebSocketClient, daemonSocketPath } from "../lib/daemon-client.mjs";

const manager = path.resolve("plugins/codex/codex-manager/manager.mjs");

function detach(home, threadId) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [manager, "detach", "--thread", threadId], {
      env: { ...process.env, CODEX_MANAGER_STATE_DIR: home, CLAUDE_CODE_SESSION_ID: "", CODEX_MANAGER_SESSIONS_DIR: path.join(home, "no-claude") },
      stdio: ["ignore", "pipe", "pipe"], timeout: 15_000
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.on("error", reject);
    child.on("close", (code) => resolve({ code, stdout, stderr }));
  });
}

test("detach CLI reports an unknown thread on one stderr line", async () => {
  const home = await mkdtemp(path.join(os.tmpdir(), "detach-cli-"));
  try {
    const result = await detach(home, "missing");
    assert.notEqual(result.code, 0);
    assert.equal(result.stdout, "");
    assert.equal(result.stderr, "unknown thread: missing\n");
  } finally {
    await rm(home, { recursive: true, force: true });
  }
});

test("detach CLI uses the owning store and live turn status after its manager exits", {
  skip: !process.env.CODEX_MANAGER_LIVE_TESTS, timeout: 60_000
}, async () => {
  const home = await mkdtemp(path.join(os.tmpdir(), "detach-cli-live-"));
  const env = { CODEX_MANAGER_STATE_DIR: home };
  const store = new SessionStore("exited-supervisor", env);
  const client = new WebSocketClient(daemonSocketPath(process.env.CODEX_DAEMON_SOCKET));
  // Observing a test thread must never answer its approvals.
  client.onServerRequest = () => {};
  let threadId;
  let turnId;
  try {
    await client.connect();
    await client.request("initialize", { clientInfo: { name: "detach-cli-test", version: "1" }, capabilities: { experimentalApi: true } });
    client.send({ method: "initialized", params: {} });
    const started = await client.request("thread/start", { cwd: home, approvalPolicy: "never", sandbox: "read-only", ephemeral: false });
    threadId = started.thread.id;
    const turn = await client.request("turn/start", { threadId, input: [{ type: "text", text: "Run sleep 120, then reply done. Do nothing else." }] });
    turnId = turn.turn.id;
    const unrelated = { id: "unrelated-thread", lastStatus: "inProgress", owner: "another-agent" };
    const save = (lastStatus, attached = false) => store.writeState({ threads: [unrelated, { id: threadId, turnId, lastStatus, attached }] });
    save("completed");
    // The owner process is gone; the calling process has no Claude session.
    writeSupervisor(threadId, { sessionId: store.sessionId, pid: 2147483647 }, env);
    const supervisorFile = path.join(home, "threads", `${threadId}.json`);
    const supervisor = await readFile(supervisorFile, "utf8");

    const running = await detach(home, threadId);
    assert.notEqual(running.code, 0);
    assert.equal(running.stdout, "");
    assert.match(running.stderr, /still running a turn/);
    assert.equal(store.readState().threads[1].detached, undefined);

    await client.request("turn/interrupt", { threadId, turnId });
    const deadline = Date.now() + 10_000;
    for (;;) {
      const turns = await client.request("thread/turns/list", { threadId, limit: 1, sortDirection: "desc", itemsView: "full" });
      if (turns.data[0].status !== "inProgress") break;
      assert.ok(Date.now() < deadline, "interrupt must finish the test turn");
      await new Promise((resolve) => setTimeout(resolve, 50));
    }

    save("inProgress", true);
    const attached = await detach(home, threadId);
    assert.equal(attached.code, 0, attached.stderr);
    const released = JSON.parse(attached.stdout);
    assert.equal(released.threadId, threadId);
    assert.equal(released.detached, true);
    assert.equal(typeof released.status, "string");
    assert.equal(released.archived, undefined);
    assert.equal(store.readState().threads[1].detached, true);
    // Unsubscribing leaves an attached thread available to its own client.
    await client.request("thread/resume", { threadId });

    save("inProgress");
    const archived = await detach(home, threadId);
    assert.deepEqual(archived, { code: 0, stdout: `${JSON.stringify({ threadId, detached: true, archived: true })}\n`, stderr: "" });
    assert.equal(store.readState().threads[1].detached, true);
    assert.deepEqual(store.readState().threads[0], unrelated);
    assert.equal(await readFile(supervisorFile, "utf8"), supervisor);
    const listed = await client.request("thread/list", { archived: true, limit: 100 });
    assert.ok(listed.data.some((thread) => thread.id === threadId));
  } finally {
    if (threadId) {
      if (turnId) await client.request("turn/interrupt", { threadId, turnId }).catch(() => {});
      await client.request("thread/archive", { threadId }).catch(() => {});
    }
    client.close();
    await rm(home, { recursive: true, force: true });
  }
});
