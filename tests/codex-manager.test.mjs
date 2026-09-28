// ABOUTME: Tests codex-manager: session resolution, the await and pending readers, and the MCP
// ABOUTME: server's daemon conversation (thread start, notify_claude, turn completion, adoption).

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { appendFile, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
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

test("whoami runs when the script is reached through a symlinked plugin directory", async () => {
  const link = await mkdtemp(path.join(os.tmpdir(), "codex-manager-link-"));
  try {
    await symlink(path.dirname(path.dirname(manager)), path.join(link, "codex"));
    const viaLink = path.join(link, "codex", "codex-manager", "codex-manager.mjs");
    const result = await new Promise((resolve) => {
      const child = spawn(process.execPath, [viaLink, "whoami"], { env: { ...process.env, CLAUDE_CODE_SESSION_ID: session }, stdio: ["ignore", "pipe", "pipe"] });
      let stdout = "";
      child.stdout.setEncoding("utf8");
      child.stdout.on("data", (chunk) => { stdout += chunk; });
      child.on("close", (code) => resolve({ code, stdout }));
    });
    assert.equal(result.code, 0);
    assert.equal(result.stdout, `${session}\n`);
  } finally {
    await rm(link, { recursive: true, force: true });
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
  constructor(home, socketPath, env = {}) {
    this.child = spawn(process.execPath, [manager, "mcp"], {
      env: { ...process.env, CLAUDE_CODE_SESSION_ID: session, CODEX_MANAGER_HOME: home, CODEX_MANAGER_DAEMON_SOCKET: socketPath, ...env },
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
  const script = {
    messages,
    sockets,
    activeTurn: undefined,
    handler(message, socket) {
      sockets.add(socket);
      messages.push(message);
      if (message.id === undefined) return;
      const reply = (result) => send(socket, { id: message.id, result });
      if (message.method === "initialize") reply({});
      if (message.method === "thread/start") reply({ thread: { id: "thread-A" } });
      if (message.method === "thread/name/set") reply({});
      if (message.method === "turn/start") {
        const steer = message.params.input[0].text.startsWith("steer:");
        if (!steer) script.activeTurn = `turn-${messages.filter((m) => m.method === "turn/start").length}`;
        reply({ turn: { id: script.activeTurn, status: "inProgress" } });
      }
      if (message.method === "turn/interrupt") reply({});
      if (message.method === "thread/resume") reply({ thread: { id: message.params.threadId }, initialTurnsPage: { data: [{ id: "turn-old", status: "completed", items: [{ type: "agentMessage", text: "done while you were away" }] }] } });
    }
  };
  return script;
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
    assert.deepEqual(tools.result.tools.map((tool) => tool.name), ["start", "send", "reply", "interrupt", "list"]);

    const started = await mcp.call("start", { cwd: "plugins", prompt: "Fix the bug", name: "fix-bug" });
    assert.equal(started.isError, false, started.text);
    assert.equal(started.json.threadId, "thread-A");
    assert.equal(started.json.turnId, "turn-1");
    assert.equal(started.json.await, `node ${JSON.stringify(manager)} await --thread thread-A`);

    const methods = script.messages.map((message) => message.method);
    assert.deepEqual(methods, ["initialize", "initialized", "thread/start", "thread/name/set", "turn/start"]);
    assert.equal(script.messages[0].params.capabilities.experimentalApi, true);
    assert.equal(script.messages[0].params.clientInfo.name, "codex-manager");
    const start = script.messages[2].params;
    assert.equal(start.cwd, path.resolve("plugins"));
    assert.equal(start.approvalPolicy, "on-request");
    assert.equal(start.sandbox, "workspace-write");
    assert.equal(start.ephemeral, false);
    assert.equal(start.serviceName, "codex-manager");
    assert.deepEqual(start.config, { "sandbox_workspace_write.network_access": true });
    assert.deepEqual(start.dynamicTools.map((tool) => [tool.type, tool.name]), [["function", "notify_claude"], ["function", "ask_claude"]]);
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

    send(socket, { id: 901, method: "item/permissions/requestApproval", params: { threadId: "thread-A" } });
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
    assert.equal(sent.json.steered, false);
    assert.equal(script.messages.at(-1).params.threadId, "thread-A");
    const interrupted = await mcp.call("interrupt", { threadId: "thread-A" });
    assert.equal(interrupted.isError, false, interrupted.text);
    assert.deepEqual(script.messages.at(-1).params, { threadId: "thread-A", turnId: "turn-2" });

    // The daemon answers a turn/start during an active turn with that turn's id: a steer.
    const fresh = await mcp.call("send", { threadId: "thread-A", prompt: "reset" });
    assert.equal(fresh.json.steered, false);
    const steered = await mcp.call("send", { threadId: "thread-A", prompt: "steer: also lint" });
    assert.equal(steered.json.steered, true);
    assert.equal(steered.json.turnId, fresh.json.turnId);
    const unknown = await mcp.call("send", { threadId: "nope", prompt: "x" });
    assert.equal(unknown.isError, true);
    assert.match(unknown.text, /unknown thread/);
    const noAsk = await mcp.call("reply", { threadId: "thread-A", text: "go" });
    assert.equal(noAsk.isError, true);
    assert.match(noAsk.text, /not waiting/);
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

async function startedThread(home, daemon, script) {
  const mcp = new McpChild(home.home, daemon.socketPath);
  await mcp.request("initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "test", version: "0" } });
  await mcp.call("start", { cwd: ".", prompt: "work" });
  await waitFor(() => script.sockets.size === 1);
  const [socket] = script.sockets;
  return { mcp, socket };
}

test("ask_claude holds the daemon request until reply answers it, and replays do not duplicate the inbox", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  const daemon = await fakeDaemon(script.handler);
  const { mcp, socket } = await startedThread(home, daemon, script);
  try {
    const ask = { threadId: "thread-A", turnId: "turn-1", callId: "call-ask", tool: "ask_claude", arguments: { text: "tests or implementation?" } };
    send(socket, { id: 910, method: "item/tool/call", params: ask });
    const inbox = path.join(home.dir, "thread-A.jsonl");
    await waitFor(() => inboxLines(inbox).some((line) => line.kind === "ask"));
    assert.equal(script.messages.some((message) => message.id === 910), false);
    const listed = await mcp.call("list", {});
    assert.deepEqual(listed.json.threads[0].waiting, [{ kind: "ask", callId: "call-ask", since: listed.json.threads[0].waiting[0].since, text: "tests or implementation?" }]);
    assert.equal(listed.json.attention.length, 1);
    assert.match(listed.json.attention[0], /thread-A/);

    // The daemon replays the same call after a resume; the inbox must not get a second ask line.
    send(socket, { id: 911, method: "item/tool/call", params: ask });
    await new Promise((resolve) => setTimeout(resolve, 200));
    assert.equal(inboxLines(inbox).filter((line) => line.kind === "ask").length, 1);

    const replied = await mcp.call("reply", { threadId: "thread-A", text: "Fix the implementation." });
    assert.equal(replied.isError, false, replied.text);
    await waitFor(() => script.messages.some((message) => message.id === 911));
    assert.deepEqual(script.messages.find((message) => message.id === 911).result, { contentItems: [{ type: "inputText", text: "Fix the implementation." }], success: true });
    assert.deepEqual(script.messages.find((message) => message.id === 910)?.result, { contentItems: [{ type: "inputText", text: "Fix the implementation." }], success: true });
    const after = await mcp.call("list", {});
    assert.deepEqual(after.json.threads[0].waiting, []);
    assert.equal(after.json.attention, undefined);
  } finally {
    await mcp.close();
    await daemon.close();
    await home.close();
  }
});

test("an unanswered ask times out with a proceed-on-your-own answer", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  const daemon = await fakeDaemon(script.handler);
  const mcp = new McpChild(home.home, daemon.socketPath, { CODEX_MANAGER_ASK_TIMEOUT: "1" });
  try {
    await mcp.request("initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "test", version: "0" } });
    await mcp.call("start", { cwd: ".", prompt: "work" });
    const [socket] = script.sockets;
    send(socket, { id: 920, method: "item/tool/call", params: { threadId: "thread-A", turnId: "turn-1", callId: "call-slow", tool: "ask_claude", arguments: { text: "?" } } });
    await waitFor(() => script.messages.some((message) => message.id === 920), { timeout: 5000 });
    const answer = script.messages.find((message) => message.id === 920).result;
    assert.equal(answer.success, true);
    assert.match(answer.contentItems[0].text, /did not answer within 1 seconds/);
    const listed = await mcp.call("list", {});
    assert.deepEqual(listed.json.threads[0].waiting, []);
  } finally {
    await mcp.close();
    await daemon.close();
    await home.close();
  }
});

test("approval requests are forwarded to Claude and answered by decision; timeouts decline", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  const daemon = await fakeDaemon(script.handler);
  const { mcp, socket } = await startedThread(home, daemon, script);
  try {
    send(socket, { id: 930, method: "item/commandExecution/requestApproval", params: { threadId: "thread-A", turnId: "turn-1", itemId: "item-cmd", approvalId: null, command: "rm -rf build", cwd: "/repo", reason: "outside the sandbox", availableDecisions: ["accept", { acceptWithExecpolicyAmendment: {} }, "decline"] } });
    const inbox = path.join(home.dir, "thread-A.jsonl");
    await waitFor(() => inboxLines(inbox).some((line) => line.kind === "approval"));
    const line = inboxLines(inbox).find((line) => line.kind === "approval");
    assert.equal(line.callId, "item-cmd");
    assert.equal(line.command, "rm -rf build");
    assert.equal(line.reason, "outside the sandbox");
    assert.deepEqual(line.decisions, ["accept", "decline"]);

    const bad = await mcp.call("reply", { threadId: "thread-A", text: "acceptForSession" });
    assert.equal(bad.isError, true);
    assert.match(bad.text, /accept, decline/);
    const ok = await mcp.call("reply", { threadId: "thread-A", text: "accept" });
    assert.equal(ok.isError, false, ok.text);
    await waitFor(() => script.messages.some((message) => message.id === 930));
    assert.deepEqual(script.messages.find((message) => message.id === 930).result, { decision: "accept" });

    send(socket, { id: 931, method: "item/fileChange/requestApproval", params: { threadId: "thread-A", turnId: "turn-1", itemId: "item-fc", reason: "writes outside cwd", grantRoot: "/etc" } });
    await waitFor(() => inboxLines(inbox).filter((line) => line.kind === "approval").length === 2);
    const fileChange = inboxLines(inbox).filter((line) => line.kind === "approval")[1];
    assert.equal(fileChange.grantRoot, "/etc");
    assert.deepEqual(fileChange.decisions, ["accept", "acceptForSession", "decline", "cancel"]);
    await mcp.call("reply", { threadId: "thread-A", text: "decline" });
    await waitFor(() => script.messages.some((message) => message.id === 931));
    assert.deepEqual(script.messages.find((message) => message.id === 931).result, { decision: "decline" });
  } finally {
    await mcp.close();
    await daemon.close();
    await home.close();
  }
});

test("pending blocks once for an unanswered ask and not again while the stop hook is active", async () => {
  const home = await tempHome();
  try {
    const env = { CODEX_MANAGER_HOME: home.home };
    await mkdir(home.dir, { recursive: true });
    await writeFile(path.join(home.dir, "state.json"), JSON.stringify({ updatedAt: 1, threads: [{ id: "thread-C", lastStatus: "inProgress", waiting: [{ kind: "ask", callId: "c", since: 1, text: "which one?" }] }] }));
    const first = await run(["pending"], { env, stdin: JSON.stringify({ session_id: session, stop_hook_active: false }) });
    assert.equal(first.code, 0, first.stderr);
    assert.match(JSON.parse(first.stdout).reason, /thread-C is waiting for reply/);
    const second = await run(["pending"], { env, stdin: JSON.stringify({ session_id: session, stop_hook_active: true }) });
    assert.equal(second.stdout, "");
  } finally {
    await home.close();
  }
});

test("mcp reconnects after the daemon drops the connection and resumes its threads", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  const daemon = await fakeDaemon(script.handler);
  const { mcp, socket } = await startedThread(home, daemon, script);
  try {
    socket.destroy();
    await waitFor(() => script.messages.some((message) => message.method === "thread/resume"), { timeout: 8000 });
    assert.equal(script.sockets.size, 2);
    const listed = await mcp.call("list", {});
    assert.equal(listed.json.connected, true);
  } finally {
    await mcp.close();
    await daemon.close();
    await home.close();
  }
});

test("a failed turn/start leaves the thread idle, and adoption backfills an unavailable thread", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  const failing = (message, socket) => {
    if (message.method === "turn/start") return send(socket, { id: message.id, error: { code: -32000, message: "draining" } });
    script.handler(message, socket);
  };
  const daemon = await fakeDaemon(failing);
  const mcp = new McpChild(home.home, daemon.socketPath);
  try {
    await mcp.request("initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "test", version: "0" } });
    const started = await mcp.call("start", { cwd: ".", prompt: "x" });
    assert.equal(started.isError, true);
    assert.match(started.text, /draining/);
    const state = JSON.parse(await readFile(path.join(home.dir, "state.json"), "utf8"));
    assert.equal(state.threads[0].lastStatus, "idle");
    assert.equal(state.threads[0].turnId, null);
    const interrupt = await mcp.call("interrupt", { threadId: "thread-A" });
    assert.match(interrupt.text, /no turn to interrupt/);
  } finally {
    await mcp.close();
    await daemon.close();
  }
  await writeFile(path.join(home.dir, "state.json"), JSON.stringify({ updatedAt: 1, threads: [{ id: "thread-B", cwd: "/tmp", turnId: null, lastStatus: "unavailable" }] }));
  const daemon2 = await fakeDaemon(script.handler);
  const mcp2 = new McpChild(home.home, daemon2.socketPath);
  try {
    await mcp2.request("initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "test", version: "0" } });
    const listed = await mcp2.call("list", {});
    assert.equal(listed.json.threads[0].lastStatus, "completed");
    assert.equal(inboxLines(path.join(home.dir, "thread-B.jsonl"))[0].kind, "completed");
  } finally {
    await mcp2.close();
    await daemon2.close();
    await home.close();
  }
});

test("server requests for threads this session does not own are rejected without touching the inbox", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  const daemon = await fakeDaemon(script.handler);
  const { mcp, socket } = await startedThread(home, daemon, script);
  try {
    send(socket, { id: 940, method: "item/tool/call", params: { threadId: "someone-elses", turnId: "t", callId: "c", tool: "notify_claude", arguments: { text: "hi" } } });
    await waitFor(() => script.messages.some((message) => message.id === 940));
    assert.equal(script.messages.find((message) => message.id === 940).error.code, -32601);
    assert.deepEqual(inboxLines(path.join(home.dir, "someone-elses.jsonl")), []);
  } finally {
    await mcp.close();
    await daemon.close();
    await home.close();
  }
});

test("an unanswered approval times out with decline", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  const daemon = await fakeDaemon(script.handler);
  const mcp = new McpChild(home.home, daemon.socketPath, { CODEX_MANAGER_ASK_TIMEOUT: "1" });
  try {
    await mcp.request("initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "test", version: "0" } });
    await mcp.call("start", { cwd: ".", prompt: "work" });
    const [socket] = script.sockets;
    send(socket, { id: 950, method: "item/fileChange/requestApproval", params: { threadId: "thread-A", turnId: "turn-1", itemId: "item-fc" } });
    await waitFor(() => script.messages.some((message) => message.id === 950), { timeout: 5000 });
    assert.deepEqual(script.messages.find((message) => message.id === 950).result, { decision: "decline" });
  } finally {
    await mcp.close();
    await daemon.close();
    await home.close();
  }
});

test("several held requests are answered by callId, and a finished turn clears what is left", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  const daemon = await fakeDaemon(script.handler);
  const { mcp, socket } = await startedThread(home, daemon, script);
  try {
    send(socket, { id: 960, method: "item/tool/call", params: { threadId: "thread-A", turnId: "turn-1", callId: "ask-1", tool: "ask_claude", arguments: { text: "first?" } } });
    send(socket, { id: 961, method: "item/commandExecution/requestApproval", params: { threadId: "thread-A", turnId: "turn-1", itemId: "cmd-1", command: "make deploy" } });
    await waitFor(() => inboxLines(path.join(home.dir, "thread-A.jsonl")).length === 2);
    const ambiguous = await mcp.call("reply", { threadId: "thread-A", text: "yes" });
    assert.equal(ambiguous.isError, true);
    assert.match(ambiguous.text, /ask-1/);
    assert.match(ambiguous.text, /cmd-1/);
    const answered = await mcp.call("reply", { threadId: "thread-A", callId: "cmd-1", text: "accept" });
    assert.equal(answered.isError, false, answered.text);
    await waitFor(() => script.messages.some((message) => message.id === 961));
    assert.deepEqual(script.messages.find((message) => message.id === 961).result, { decision: "accept" });
    assert.equal(script.messages.some((message) => message.id === 960), false);

    send(socket, { method: "turn/completed", params: { threadId: "thread-A", turn: { id: "turn-1", status: "interrupted" } } });
    await waitFor(() => inboxLines(path.join(home.dir, "thread-A.jsonl")).some((line) => line.kind === "completed"));
    const listed = await mcp.call("list", {});
    assert.deepEqual(listed.json.threads[0].waiting, []);
    assert.equal(listed.json.attention, undefined);
  } finally {
    await mcp.close();
    await daemon.close();
    await home.close();
  }
});

test("reconnect keeps retrying while the daemon is down and does not redeliver a known completion", { timeout: 30_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  const socketPath = path.join(home.home, "daemon.sock");
  const daemon = await fakeDaemon(script.handler, () => {}, { socketPath });
  const { mcp, socket } = await startedThread(home, daemon, script);
  try {
    // Deliver turn-1 as completed, then take the daemon away for longer than the first retry delay.
    send(socket, { method: "turn/completed", params: { threadId: "thread-A", turn: { id: "turn-1", status: "completed" } } });
    await waitFor(() => inboxLines(path.join(home.dir, "thread-A.jsonl")).length === 1);
    await daemon.close();
    await rm(socketPath, { force: true });
    await new Promise((resolve) => setTimeout(resolve, 2500));
    const revived = { ...script, handler(message, socket) {
      if (message.id !== undefined && message.method === "thread/resume") {
        script.messages.push(message);
        return send(socket, { id: message.id, result: { thread: { id: "thread-A" }, initialTurnsPage: { data: [{ id: "turn-1", status: "completed", items: [] }] } } });
      }
      script.handler(message, socket);
    } };
    const again = await fakeDaemon(revived.handler, () => {}, { socketPath });
    try {
      await waitFor(() => script.messages.filter((message) => message.method === "thread/resume").length === 1, { timeout: 15000 });
      const listed = await mcp.call("list", {});
      assert.equal(listed.json.connected, true);
      assert.equal(inboxLines(path.join(home.dir, "thread-A.jsonl")).length, 1);
    } finally {
      await again.close();
    }
  } finally {
    await mcp.close();
    await home.close();
  }
});
