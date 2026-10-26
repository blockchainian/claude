import assert from "node:assert/strict";
import { linkSync, mkdtempSync, mkdirSync, readdirSync, readFileSync, rmSync, watch, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import test from "node:test";
import * as inbox from "../lib/inbox.mjs";
import { SessionStore } from "../lib/inbox.mjs";

for (const state of ["missing", "empty", "consumed", "partial", "pending skips await"]) {
  test(`claim creates no lock directory when inbox is ${state}`, async () => {
    const root = mkdtempSync(path.join(os.tmpdir(), "codex-inbox-"));
    const store = new SessionStore("session", { CODEX_MANAGER_STATE_DIR: root });
    mkdirSync(store.dir);
    if (state === "empty") writeFileSync(store.inboxPath("thread"), "");
    if (state === "partial") writeFileSync(store.inboxPath("thread"), '{"kind":');
    if (state === "consumed" || state === "pending skips await") {
      store.append("thread", { kind: "notify", text: "delivered" });
      store.advance("thread", store.inboxSize("thread"));
    }
    const events = [];
    const watcher = watch(store.dir, (event, filename) => events.push(String(filename)));
    try {
      for (let poll = 0; poll < 3; poll += 1) {
        assert.deepEqual(store.claim("thread", state === "pending skips await" ? "pending" : undefined), []);
        await sleep(30);
      }
      assert.deepEqual(events.filter((name) => name.endsWith(".lock")), []);
    } finally {
      watcher.close();
      rmSync(root, { recursive: true, force: true });
    }
  });
}


test("thread records replace files atomically without leaving temporary files", () => {
  const root = mkdtempSync(path.join(os.tmpdir(), "codex-record-"));
  const env = { CODEX_MANAGER_STATE_DIR: root };
  const directory = path.join(root, "threads");
  const target = path.join(directory, "thread.json");
  const previous = path.join(root, "previous.json");
  try {
    mkdirSync(directory);
    writeFileSync(target, "{}");
    linkSync(target, previous);
    const record = { sessionId: "session", pid: process.pid };
    inbox.writeThreadRecord("thread", record, env);
    assert.deepEqual(inbox.readThreadRecord("thread", env), record);
    assert.equal(readFileSync(previous, "utf8"), "{}");
    assert.deepEqual(readdirSync(directory), ["thread.json"]);
    inbox.writeThreadRecord("thread", {}, env);
    assert.deepEqual(inbox.readThreadRecord("thread", env), {});
    assert.deepEqual(readdirSync(directory), ["thread.json"]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
