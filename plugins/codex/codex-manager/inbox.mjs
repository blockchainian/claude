// ABOUTME: Per-session storage for codex-manager: state.json, one inbox per codex thread, reader-owned
// ABOUTME: cursors so events reach Claude exactly once, and the questions codex is waiting on.

import { appendFileSync, existsSync, mkdirSync, readdirSync, readFileSync, renameSync, rmSync, statSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";

export function managerHome(env = process.env) {
  return env.CODEX_MANAGER_HOME || path.join(os.homedir(), ".claude", "codex-manager");
}

export const askTimeoutSeconds = (env = process.env) => Number(env.CODEX_MANAGER_ASK_TIMEOUT) > 0 ? Number(env.CODEX_MANAGER_ASK_TIMEOUT) : 300;

function supervisorPath(threadId, env) {
  return path.join(managerHome(env), "threads", `${threadId}.json`);
}

function readJson(file) {
  try {
    return JSON.parse(readFileSync(file, "utf8"));
  } catch {
    return undefined;
  }
}

/** Names the Claude session, and its manager process, that a codex thread reports to; the last one to claim a thread has it. */
export function writeSupervisor(threadId, supervisor, env = process.env) {
  const target = supervisorPath(threadId, env);
  mkdirSync(path.dirname(target), { recursive: true });
  writeFileSync(target, JSON.stringify(supervisor));
}

export function readSupervisor(threadId, env = process.env) {
  return readJson(supervisorPath(threadId, env));
}

export class SessionStore {
  constructor(sessionId, env = process.env) {
    this.sessionId = sessionId;
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

  /**
   * Takes the unread lines and advances the cursor under a lock, so `await` and the Stop hook
   * never deliver the same event twice. Returns [] when the lock is busy or nothing is unread.
   */
  claim(threadId) {
    const lock = `${this.inboxPath(threadId)}.lock`;
    if (!this.acquire(lock)) return [];
    try {
      const { lines, end } = this.unread(threadId);
      if (lines.length) this.advance(threadId, end);
      return lines;
    } finally {
      rmSync(lock, { recursive: true, force: true });
    }
  }

  acquire(lock) {
    mkdirSync(this.dir, { recursive: true });
    for (let attempt = 0; attempt < 40; attempt += 1) {
      try {
        mkdirSync(lock);
        return true;
      } catch {
        // A holder that died mid-claim must not block readers forever.
        try {
          if (Date.now() - statSync(lock).mtimeMs > 5000) rmSync(lock, { recursive: true, force: true });
        } catch {
          // lock vanished between the failed mkdir and the stat; retry
        }
        Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 25);
      }
    }
    return false;
  }

  askPath(threadId, callId) {
    return path.join(this.dir, `${threadId}.ask.${encodeURIComponent(callId)}.json`);
  }

  replyPath(threadId, callId) {
    return path.join(this.dir, `${threadId}.reply.${encodeURIComponent(callId)}.json`);
  }

  /** A question stays on disk while codex waits for its answer; only the process that asked writes it. */
  writeAsk(threadId, ask) {
    mkdirSync(this.dir, { recursive: true });
    writeFileSync(this.askPath(threadId, ask.callId), JSON.stringify(ask));
  }

  hasAsk(threadId, callId) {
    return existsSync(this.askPath(threadId, callId));
  }

  removeAsk(threadId, callId) {
    rmSync(this.askPath(threadId, callId), { force: true });
  }

  /** The questions of a thread that nobody has answered yet, oldest first. */
  asks(threadId) {
    if (!this.exists()) return [];
    const prefix = `${threadId}.ask.`;
    return readdirSync(this.dir)
      .filter((name) => name.startsWith(prefix))
      .map((name) => readJson(path.join(this.dir, name)))
      .filter(Boolean)
      .sort((first, second) => first.since - second.since);
  }

  /** The reply is complete before the question goes, so the asker never finds neither. */
  writeReply(threadId, callId, text) {
    const target = this.replyPath(threadId, callId);
    writeFileSync(`${target}.tmp`, JSON.stringify({ text }));
    renameSync(`${target}.tmp`, target);
    this.removeAsk(threadId, callId);
  }

  takeReply(threadId, callId) {
    const reply = readJson(this.replyPath(threadId, callId));
    if (reply) rmSync(this.replyPath(threadId, callId), { force: true });
    return reply;
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
