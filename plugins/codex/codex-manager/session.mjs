// ABOUTME: Resolves which Claude Code session a codex-manager process belongs to.
// ABOUTME: Uses the environment when present, else the parent claude process's session file.

import { execFileSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

export function defaultSessionsDir() {
  return path.join(os.homedir(), ".claude", "sessions");
}

/** Parent chain of this process, nearest first, as `{pid, comm}` rows. */
export function parentProcesses(pid = process.pid) {
  const rows = [];
  while (pid > 1) {
    let line;
    try {
      line = execFileSync("ps", ["-o", "ppid=,comm=", "-p", String(pid)], { encoding: "utf8" }).trim();
    } catch {
      break;
    }
    const match = line.match(/^\s*(\d+)\s+(.*)$/);
    if (!match) break;
    pid = Number(match[1]);
    rows.push({ pid, comm: match[2] });
  }
  return rows;
}

/**
 * The session file is keyed by the pid Claude Code registered, which may be a shell wrapper
 * rather than a process named claude, so every ancestor is tried in order.
 */
export async function resolveSessionId({ env = process.env, sessionsDir = defaultSessionsDir(), parents } = {}) {
  if (env.CLAUDE_CODE_SESSION_ID) return env.CLAUDE_CODE_SESSION_ID;
  for (const { pid } of parents ?? parentProcesses()) {
    try {
      const record = JSON.parse(await readFile(path.join(sessionsDir, `${pid}.json`), "utf8"));
      if (record.sessionId) return record.sessionId;
    } catch {
      // not this ancestor
    }
  }
  throw new Error("cannot determine the Claude session: set CLAUDE_CODE_SESSION_ID or run under Claude Code");
}
