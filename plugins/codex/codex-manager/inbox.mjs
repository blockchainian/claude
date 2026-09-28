// ABOUTME: Per-session storage for codex-manager: state.json, one inbox per codex thread,
// ABOUTME: and reader-owned cursors so events are delivered to Claude exactly once.

import { appendFileSync, existsSync, mkdirSync, readdirSync, readFileSync, renameSync, statSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";

export function managerHome(env = process.env) {
  return env.CODEX_MANAGER_HOME || path.join(os.homedir(), ".claude", "codex-manager");
}

export class SessionStore {
  constructor(sessionId, env = process.env) {
    this.dir = path.join(managerHome(env), sessionId);
  }

  exists() {
    return existsSync(this.dir);
  }

  inboxPath(threadId) {
    return path.join(this.dir, `${threadId}.jsonl`);
  }

  cursorPath(threadId) {
    return path.join(this.dir, `${threadId}.cursor`);
  }

  readState() {
    try {
      return JSON.parse(readFileSync(path.join(this.dir, "state.json"), "utf8"));
    } catch {
      return { updatedAt: 0, threads: [] };
    }
  }

  /** Replaces state.json atomically so readers never see a partial file. */
  writeState(state) {
    mkdirSync(this.dir, { recursive: true });
    const target = path.join(this.dir, "state.json");
    const temporary = `${target}.${process.pid}.tmp`;
    writeFileSync(temporary, JSON.stringify({ ...state, updatedAt: Date.now() }, null, 2));
    renameSync(temporary, target);
  }

  append(threadId, event) {
    mkdirSync(this.dir, { recursive: true });
    appendFileSync(this.inboxPath(threadId), `${JSON.stringify({ ts: Date.now(), ...event })}\n`);
  }

  readCursor(threadId) {
    try {
      return Number(readFileSync(this.cursorPath(threadId), "utf8")) || 0;
    } catch {
      return 0;
    }
  }

  /** Unread complete lines after the cursor, and the offset just past the last of them. */
  unread(threadId) {
    const cursor = this.readCursor(threadId);
    let text;
    try {
      text = readFileSync(this.inboxPath(threadId));
    } catch {
      return { lines: [], end: cursor };
    }
    const fresh = text.subarray(cursor);
    const lastNewline = fresh.lastIndexOf("\n");
    if (lastNewline === -1) return { lines: [], end: cursor };
    const lines = fresh.subarray(0, lastNewline).toString("utf8").split("\n");
    return { lines, end: cursor + lastNewline + 1 };
  }

  unreadCount(threadId) {
    return this.unread(threadId).lines.length;
  }

  /** Marks everything before `end` as delivered; only readers call this. */
  advance(threadId, end) {
    writeFileSync(this.cursorPath(threadId), String(end));
  }

  threadIds() {
    if (!this.exists()) return [];
    return readdirSync(this.dir).filter((name) => name.endsWith(".jsonl")).map((name) => name.slice(0, -".jsonl".length));
  }

  inboxSize(threadId) {
    try {
      return statSync(this.inboxPath(threadId)).size;
    } catch {
      return 0;
    }
  }
}
