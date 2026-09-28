// ABOUTME: Tests codex-manager: session resolution, the await and pending readers, and the MCP
// ABOUTME: server's daemon conversation (thread start, notify_claude, turn completion, adoption).

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { appendFile, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import test from "node:test";
import { fakeDaemon, send } from "./helpers/fake-daemon.mjs";
import { resolveSessionId } from "../plugins/codex/codex-manager/session.mjs";

const manager = path.resolve("plugins/codex/codex-manager/codex-manager.mjs");
const session = "11111111-2222-3333-4444-555555555555";

async function tempHome() {
  const home = await mkdtemp(path.join(os.tmpdir(), "codex-manager-test-"));
  return { home, dir: path.join(home, session), async close() { await rm(home, { recursive: true, force: true }); } };
}

function run(args, { env = {}, stdin } = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [manager, ...args], {
      env: { ...process.env, CLAUDE_CODE_SESSION_ID: session, ...env },
      stdio: ["pipe", "pipe", "pipe"]
    });
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.on("error", reject);
    child.on("close", (code) => resolve({ code, stdout, stderr }));
    if (stdin !== undefined) child.stdin.end(stdin); else child.stdin.end();
  });
}

test("resolveSessionId prefers the environment, then the parent claude session file", async () => {
  const sessions = await mkdtemp(path.join(os.tmpdir(), "claude-sessions-"));
  try {
    await writeFile(path.join(sessions, "4242.json"), JSON.stringify({ pid: 4242, sessionId: "from-file" }));
    const parents = [{ pid: 7, comm: "zsh" }, { pid: 4242, comm: "claude" }, { pid: 1, comm: "launchd" }];
    assert.equal(await resolveSessionId({ env: { CLAUDE_CODE_SESSION_ID: "from-env" }, sessionsDir: sessions, parents }), "from-env");
    assert.equal(await resolveSessionId({ env: {}, sessionsDir: sessions, parents }), "from-file");
    await assert.rejects(resolveSessionId({ env: {}, sessionsDir: sessions, parents: [{ pid: 7, comm: "zsh" }] }), /cannot determine the Claude session/);
  } finally {
    await rm(sessions, { recursive: true, force: true });
  }
});

test("await prints unread lines, advances the cursor, and times out on silence", { timeout: 15_000 }, async () => {
  const home = await tempHome();
  try {
    await mkdir(home.dir, { recursive: true });
    const inbox = path.join(home.dir, "thread-1.jsonl");
    await writeFile(inbox, '{"kind":"notify","text":"first"}\n{"kind":"completed","status":"completed"}\n');
    const env = { CODEX_MANAGER_HOME: home.home };
    const first = await run(["await", "--thread", "thread-1"], { env });
    assert.equal(first.code, 0, first.stderr);
    assert.equal(first.stdout, '{"kind":"notify","text":"first"}\n{"kind":"completed","status":"completed"}\n');
    assert.equal(await readFile(path.join(home.dir, "thread-1.cursor"), "utf8"), String(Buffer.byteLength(first.stdout)));
    const quiet = await run(["await", "--thread", "thread-1", "--timeout", "1"], { env });
    assert.equal(quiet.code, 124);
    assert.equal(quiet.stdout, "");
    const later = run(["await", "--thread", "thread-1", "--timeout", "5"], { env });
    setTimeout(() => appendFile(inbox, '{"kind":"notify","text":"second"}\n'), 400);
    const result = await later;
    assert.equal(result.code, 0, result.stderr);
    assert.equal(result.stdout, '{"kind":"notify","text":"second"}\n');
  } finally {
    await home.close();
  }
});

test("pending stays silent without state and blocks once on unread events", async () => {
  const home = await tempHome();
  try {
    const env = { CODEX_MANAGER_HOME: home.home };
    const hookInput = JSON.stringify({ session_id: session, stop_hook_active: false });
    const nothing = await run(["pending"], { env, stdin: hookInput });
    assert.equal(nothing.code, 0);
    assert.equal(nothing.stdout, "");
    await mkdir(home.dir, { recursive: true });
    await writeFile(path.join(home.dir, "thread-9.jsonl"), '{"kind":"completed","status":"failed"}\n');
    const blocked = await run(["pending"], { env, stdin: hookInput });
    assert.equal(blocked.code, 0, blocked.stderr);
    const decision = JSON.parse(blocked.stdout);
    assert.equal(decision.decision, "block");
    assert.match(decision.reason, /thread-9/);
    assert.match(decision.reason, /"kind":"completed"/);
    const again = await run(["pending"], { env, stdin: hookInput });
    assert.equal(again.stdout, "");
  } finally {
    await home.close();
  }
});

class McpChild {
  constructor(home, socketPath) {
    this.child = spawn(process.execPath, [manager, "mcp"], {
      env: { ...process.env, CLAUDE_CODE_SESSION_ID: session, CODEX_MANAGER_HOME: home, CODEX_MANAGER_DAEMON_SOCKET: socketPath },
      stdio: ["pipe", "pipe", "pipe"]
    });
    this.stderr = "";
    this.child.stderr.setEncoding("utf8");
    this.child.stderr.on("data", (chunk) => { this.stderr += chunk; });
    this.buffer = "";
    this.pending = new Map();
    this.nextId = 1;
    this.child.stdout.setEncoding("utf8");
    this.child.stdout.on("data", (chunk) => {
      this.buffer += chunk;
      let index;
      while ((index = this.buffer.indexOf("\n")) !== -1) {
        const line = this.buffer.slice(0, index);
        this.buffer = this.buffer.slice(index + 1);
        if (!line.trim()) continue;
        const message = JSON.parse(line);
        this.pending.get(message.id)?.(message);
        this.pending.delete(message.id);
      }
    });
  }

  request(method, params) {
    const id = this.nextId++;
    this.child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
    return new Promise((resolve) => this.pending.set(id, resolve));
  }

  async call(name, args) {
    const response = await this.request("tools/call", { name, arguments: args });
    assert.equal(response.error, undefined, JSON.stringify(response));
    const text = response.result.content[0].text;
    return { isError: response.result.isError === true, text, json: response.result.isError ? undefined : JSON.parse(text) };
  }

  async close() {
    this.child.stdin.end();
    await new Promise((resolve) => this.child.on("close", resolve));
  }
}

function daemonScript() {
  const messages = [];
  const sockets = new Set();
  return {
    messages,
    sockets,
    handler(message, socket) {
      sockets.add(socket);
      messages.push(message);
      if (message.id === undefined) return;
      const reply = (result) => send(socket, { id: message.id, result });
      if (message.method === "initialize") reply({});
      if (message.method === "thread/start") reply({ thread: { id: "thread-A" } });
      if (message.method === "thread/name/set") reply({});
      if (message.method === "turn/start") reply({ turn: { id: `turn-${messages.filter((m) => m.method === "turn/start").length}`, status: "inProgress" } });
      if (message.method === "turn/interrupt") reply({});
      if (message.method === "thread/resume") reply({ thread: { id: message.params.threadId }, initialTurnsPage: { data: [{ id: "turn-old", status: "completed", items: [{ type: "agentMessage", text: "done while you were away" }] }] } });
    }
  };
}

function inboxLines(file) {
  let text = "";
  try { text = readFileSync(file, "utf8"); } catch { return []; }
  return text.split("\n").filter(Boolean).map((line) => JSON.parse(line));
}

function waitFor(predicate, { timeout = 5000 } = {}) {
  return new Promise((resolve, reject) => {
    const started = Date.now();
    const tick = () => {
      if (predicate()) return resolve();
      if (Date.now() - started > timeout) return reject(new Error("condition not met in time"));
      setTimeout(tick, 20);
    };
    tick();
  });
}

test("mcp starts a thread with dynamic tools, relays notify_claude and completion into the inbox", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  const daemon = await fakeDaemon(script.handler);
  const mcp = new McpChild(home.home, daemon.socketPath);
  try {
    const init = await mcp.request("initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "test", version: "0" } });
    assert.equal(init.result.serverInfo.name, "codex-manager");
    const tools = await mcp.request("tools/list", {});
    assert.deepEqual(tools.result.tools.map((tool) => tool.name), ["start", "send", "interrupt", "list"]);

    const started = await mcp.call("start", { cwd: "plugins", prompt: "Fix the bug", name: "fix-bug" });
    assert.equal(started.isError, false, started.text);
    assert.equal(started.json.threadId, "thread-A");
    assert.equal(started.json.turnId, "turn-1");
    assert.match(started.json.await, new RegExp(`^node ${manager.replaceAll(".", "\\.")} await --thread thread-A$`));

    const methods = script.messages.map((message) => message.method);
    assert.deepEqual(methods, ["initialize", "initialized", "thread/start", "thread/name/set", "turn/start"]);
    assert.equal(script.messages[0].params.capabilities.experimentalApi, true);
    assert.equal(script.messages[0].params.clientInfo.name, "codex-manager");
    const start = script.messages[2].params;
    assert.equal(start.cwd, path.resolve("plugins"));
    assert.equal(start.approvalPolicy, "never");
    assert.equal(start.sandbox, "workspace-write");
    assert.equal(start.ephemeral, false);
    assert.equal(start.serviceName, "codex-manager");
    assert.deepEqual(start.config, { "sandbox_workspace_write.network_access": true });
    assert.equal(start.dynamicTools.length, 1);
    assert.equal(start.dynamicTools[0].type, "function");
    assert.equal(start.dynamicTools[0].name, "notify_claude");
    assert.deepEqual(start.dynamicTools[0].inputSchema.required, ["text"]);
    assert.deepEqual(script.messages[3].params, { threadId: "thread-A", name: "fix-bug" });
    assert.equal(script.messages[4].params.input[0].text, "Fix the bug");

    const state = JSON.parse(await readFile(path.join(home.dir, "state.json"), "utf8"));
    assert.equal(state.threads[0].id, "thread-A");
    assert.equal(state.threads[0].lastStatus, "inProgress");
    assert.equal(state.threads[0].cwd, path.resolve("plugins"));

    const [socket] = script.sockets;
    send(socket, { id: 900, method: "item/tool/call", params: { threadId: "thread-A", turnId: "turn-1", callId: "call-1", tool: "notify_claude", arguments: { text: "tests are red" } } });
    await waitFor(() => script.messages.some((message) => message.id === 900));
    const answer = script.messages.find((message) => message.id === 900);
    assert.deepEqual(answer.result, { contentItems: [{ type: "inputText", text: "Delivered to Claude." }], success: true });

    send(socket, { id: 901, method: "item/commandExecution/requestApproval", params: { threadId: "thread-A" } });
    await waitFor(() => script.messages.some((message) => message.id === 901));
    assert.equal(script.messages.find((message) => message.id === 901).error.code, -32601);

    send(socket, { method: "item/completed", params: { threadId: "thread-A", turnId: "turn-1", item: { type: "agentMessage", id: "m1", text: "All fixed" } } });
    send(socket, { method: "turn/completed", params: { threadId: "thread-A", turn: { id: "turn-1", status: "completed" } } });
    const inbox = path.join(home.dir, "thread-A.jsonl");
    await waitFor(() => inboxLines(inbox).length === 2);
    const lines = inboxLines(inbox);
    assert.equal(lines[0].kind, "notify");
    assert.equal(lines[0].text, "tests are red");
    assert.equal(lines[0].turnId, "turn-1");
    assert.equal(lines[1].kind, "completed");
    assert.equal(lines[1].status, "completed");
    assert.equal(lines[1].lastMessage, "All fixed");

    const listed = await mcp.call("list", {});
    assert.equal(listed.json.threads[0].unread, 2);
    assert.equal(listed.json.threads[0].lastStatus, "completed");

    const sent = await mcp.call("send", { threadId: "thread-A", prompt: "Now add a test" });
    assert.equal(sent.json.turnId, "turn-2");
    assert.equal(script.messages.at(-1).params.threadId, "thread-A");
    const interrupted = await mcp.call("interrupt", { threadId: "thread-A" });
    assert.equal(interrupted.isError, false, interrupted.text);
    assert.deepEqual(script.messages.at(-1).params, { threadId: "thread-A", turnId: "turn-2" });

    const unknown = await mcp.call("send", { threadId: "nope", prompt: "x" });
    assert.equal(unknown.isError, true);
    assert.match(unknown.text, /unknown thread/);
  } finally {
    await mcp.close();
    await daemon.close();
    await home.close();
  }
});

test("mcp adopts recorded threads on startup and backfills turns that finished meanwhile", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  await mkdir(home.dir, { recursive: true });
  await writeFile(path.join(home.dir, "state.json"), JSON.stringify({ updatedAt: 1, threads: [{ id: "thread-B", name: "old", cwd: "/tmp", turnId: "turn-old", lastStatus: "inProgress" }] }));
  const script = daemonScript();
  const daemon = await fakeDaemon(script.handler);
  const mcp = new McpChild(home.home, daemon.socketPath);
  try {
    await mcp.request("initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "test", version: "0" } });
    const listed = await mcp.call("list", {});
    assert.equal(listed.json.threads[0].lastStatus, "completed");
    const resume = script.messages.find((message) => message.method === "thread/resume");
    assert.deepEqual(resume.params, { threadId: "thread-B", initialTurnsPage: { limit: 1, sortDirection: "desc" } });
    const lines = inboxLines(path.join(home.dir, "thread-B.jsonl"));
    assert.equal(lines.length, 1);
    assert.equal(lines[0].kind, "completed");
    assert.equal(lines[0].turnId, "turn-old");
    assert.equal(lines[0].lastMessage, "done while you were away");
  } finally {
    await mcp.close();
    await daemon.close();
    await home.close();
  }
});

test("mcp reports a daemon that is not running as a tool error, not a crash", { timeout: 10_000 }, async () => {
  const home = await tempHome();
  const mcp = new McpChild(home.home, path.join(home.home, "missing.sock"));
  try {
    await mcp.request("initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "test", version: "0" } });
    const started = await mcp.call("start", { cwd: ".", prompt: "x" });
    assert.equal(started.isError, true);
    assert.match(started.text, /cannot connect to daemon/);
    const listed = await mcp.call("list", {});
    assert.equal(listed.isError, false);
    assert.equal(listed.json.connected, false);
  } finally {
    await mcp.close();
    await home.close();
  }
});
