import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, watch, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import test from "node:test";
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
