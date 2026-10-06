import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";
import test from "node:test";

test("the packaged Claude client starts without a Claude host and delivers real MCP notifications", { timeout: 10_000 }, async () => {
  const root = path.resolve("plugins/codex");
  const manifest = JSON.parse(await readFile(path.join(root, ".codex-plugin/plugin.json"), "utf8"));
  const server = manifest.mcpServers.claude;
  const state = await mkdtemp(path.join(os.tmpdir(), "claude-client-"));
  const child = spawn(server.command, server.args, {
    cwd: path.resolve(root, server.cwd),
    env: { ...process.env, CLAUDE_CODE_SESSION_ID: "", CODEX_MANAGER_STATE_DIR: state },
    stdio: ["pipe", "pipe", "pipe"],
  });
  const closed = once(child, "close");
  const pending = new Map();
  let nextId = 0;
  let stderr = "";
  child.stderr.on("data", (chunk) => { stderr += chunk; });
  child.on("error", (error) => {
    for (const { reject } of pending.values()) reject(error);
    pending.clear();
  });
  child.on("close", () => {
    for (const { reject } of pending.values()) reject(new Error(`MCP exited: ${stderr}`));
    pending.clear();
  });
  const lines = readline.createInterface({ input: child.stdout });
  lines.on("line", (line) => {
    const message = JSON.parse(line);
    pending.get(message.id)?.resolve(message);
    pending.delete(message.id);
  });
  function request(method, params) {
    const id = ++nextId;
    return new Promise((resolve, reject) => {
      pending.set(id, { resolve, reject });
      child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
    });
  }
  const deadline = setTimeout(() => child.kill(), 8_000);
  try {
    const initialized = await request("initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "claude-client-test", version: "1" } });
    assert.equal(initialized.result.serverInfo.name, "claude");
    assert.equal(initialized.result.serverInfo.version, manifest.version);
    child.stdin.write('{"jsonrpc":"2.0","method":"notifications/initialized"}\n');
    const listed = await request("tools/list", {});
    assert.deepEqual(listed.result.tools.map(({ name }) => name).sort(), ["ask_claude", "notify_claude"]);
    const call = (name, text, threadId) => request("tools/call", { name, arguments: { text }, _meta: { threadId, callId: "client-call" } });
    const orphan = await call("ask_claude", "Who supervises this thread?", "unclaimed");
    assert.equal(orphan.result.isError, true);
    assert.match(orphan.result.content[0].text, /No Claude session is supervising/);
    await mkdir(path.join(state, "threads"));
    await writeFile(path.join(state, "threads", "client-thread.json"), JSON.stringify({ sessionId: "supervisor", pid: process.pid }));
    const delivered = await call("notify_claude", "Client installation verified.", "client-thread");
    assert.equal(delivered.result.isError, undefined);
    assert.equal(delivered.result.content[0].text, "Delivered to Claude.");
    const event = JSON.parse((await readFile(path.join(state, "supervisor", "client-thread.jsonl"), "utf8")).trim());
    assert.equal(event.kind, "notify");
    assert.equal(event.text, "Client installation verified.");
  } finally {
    clearTimeout(deadline);
    child.kill();
    await closed;
    lines.close();
    await rm(state, { recursive: true, force: true });
  }
});
