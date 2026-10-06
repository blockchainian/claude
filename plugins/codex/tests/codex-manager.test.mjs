// ABOUTME: Tests codex-manager: session resolution, the await and pending readers, and the MCP
// ABOUTME: server's daemon conversation (thread start and attach, turn completion, adoption), and the tools codex calls.

import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { appendFile, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import test from "node:test";
import { fakeDaemon, send } from "./helpers/fake-daemon.mjs";
import { parentProcesses, resolveSessionId } from "../codex-manager/session.mjs";

const manager = path.resolve("plugins/codex/codex-manager/manager.mjs");
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
    // Claude Code registers the pid it was launched as, which can be a shell wrapper, not `claude`.
    await writeFile(path.join(sessions, "4242.json"), JSON.stringify({ pid: 4242, sessionId: "from-file" }));
    const parents = [{ pid: 7, comm: "node" }, { pid: 4242, comm: "/bin/zsh" }, { pid: 1827, comm: "claude" }, { pid: 1, comm: "launchd" }];
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
    const viaLink = path.join(link, "codex", "codex-manager", "manager.mjs");
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
    const env = { CODEX_MANAGER_STATE_DIR: home.home };
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
    const env = { CODEX_MANAGER_STATE_DIR: home.home };
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
  constructor(home, socketPath, env = {}, command = "mcp") {
    this.child = spawn(process.execPath, [manager, command], {
      env: { ...process.env, CLAUDE_CODE_SESSION_ID: session, CODEX_MANAGER_STATE_DIR: home, CODEX_MANAGER_SESSIONS_DIR: path.join(home, "sessions"), CODEX_DAEMON_SOCKET: socketPath, ...env },
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

  async call(name, args, meta) {
    const response = await this.request("tools/call", { name, arguments: args, _meta: meta });
    assert.equal(response.error, undefined, JSON.stringify(response));
    const text = response.result.content[0].text;
    let json;
    try { json = JSON.parse(text); } catch { json = undefined; }
    return { isError: response.result.isError === true, text, json };
  }

  async close() {
    this.child.stdin.end();
    await new Promise((resolve) => this.child.on("close", resolve));
  }
}

/** The server codex runs for notify_claude and ask_claude; it gets no Claude session id of its own. */
class ToolsChild extends McpChild {
  constructor(home, env = {}) {
    super(home, "unused", { CLAUDE_CODE_SESSION_ID: "", ...env }, "claude");
  }

  tool(name, threadId, callId, text) {
    const meta = { callId, threadId, "x-codex-turn-metadata": { thread_id: threadId, turn_id: "turn-1" } };
    return this.call(name, { text }, threadId === undefined ? undefined : meta);
  }
}

function daemonScript() {
  const messages = [];
  const sockets = new Set();
  const script = {
    messages,
    sockets,
    activeTurn: undefined,
    threads: [],
    pageSize: 2,
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
      if (message.method === "review/start") reply({ turn: { id: "turn-review", status: "inProgress" }, reviewThreadId: message.params.threadId });
      if (message.method === "thread/unsubscribe") reply({ status: "unsubscribed" });
      if (message.method === "thread/resume") reply({ thread: { id: message.params.threadId } });
      if (message.method === "thread/turns/list") reply({ data: script.resumeTurns ?? [{ id: "turn-old", status: "completed", items: script.resumeItems ?? [{ type: "agentMessage", text: "done while you were away" }] }] });
      if (message.method === "thread/list") {
        const from = Number(message.params.cursor ?? 0);
        const next = from + script.pageSize;
        reply({ data: script.threads.slice(from, next), nextCursor: next < script.threads.length ? String(next) : null });
      }
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

test("mcp initialize provides thread lifecycle instructions before tools are loaded", async () => {
  const home = await tempHome();
  try {
    const response = await run(["mcp"], {
      env: { CODEX_MANAGER_STATE_DIR: home.home },
      stdin: `${JSON.stringify({ jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-06-18" } })}\n`
    });
    assert.equal(response.code, 0, response.stderr);
    const { result } = JSON.parse(response.stdout);
    assert.equal(typeof result.instructions, "string");
    assert.match(result.instructions, /start.*await.*background/);
    assert.match(result.instructions, /reply.*already answered.*list.*waiting/);
    assert.match(result.instructions, /finished only.*turn completed.*not failed, interrupted.*waiting on an ask.*checked and accepted.*no further message/);
    assert.match(result.instructions, /Blocked threads.*results still being checked.*not finished/);
    assert.match(result.instructions, /detach.*same turn.*send re-attaches.*safe/);
    assert.match(result.instructions, /Before ending a multi-thread run, list.*detach every finished thread/);
  } finally {
    await home.close();
  }
});

test("mcp starts a thread that reaches Claude's tools over MCP, and relays completion into the inbox", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  const daemon = await fakeDaemon(script.handler);
  const mcp = new McpChild(home.home, daemon.socketPath);
  try {
    const init = await mcp.request("initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "test", version: "0" } });
    assert.equal(init.result.serverInfo.name, "codex-manager");
    const tools = await mcp.request("tools/list", {});
    assert.deepEqual(tools.result.tools.map((tool) => tool.name), ["start", "attach", "send", "reply", "interrupt", "list", "detach", "review"]);

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
    assert.deepEqual(start.config, {
      "sandbox_workspace_write.network_access": true,
      "mcp_servers.claude-client": { command: process.execPath, args: [manager, "claude"], env: { CODEX_MANAGER_STATE_DIR: home.home }, tool_timeout_sec: 360, default_tools_approval_mode: "approve" }
    });
    assert.equal(start.dynamicTools, undefined);
    assert.deepEqual(JSON.parse(await readFile(path.join(home.home, "threads", "thread-A.json"), "utf8")), { sessionId: session, pid: mcp.child.pid });
    assert.deepEqual(script.messages[3].params, { threadId: "thread-A", name: "fix-bug" });
    assert.equal(script.messages[4].params.input[0].text, "Fix the bug");

    const state = JSON.parse(await readFile(path.join(home.dir, "state.json"), "utf8"));
    assert.equal(state.threads[0].id, "thread-A");
    assert.equal(state.threads[0].lastStatus, "inProgress");
    assert.equal(state.threads[0].cwd, path.resolve("plugins"));

    const [socket] = script.sockets;
    send(socket, { id: 900, method: "item/tool/call", params: { threadId: "thread-A", turnId: "turn-1", callId: "call-1", tool: "notify_claude", arguments: { text: "tests are red" } } });
    await waitFor(() => script.messages.some((message) => message.id === 900));
    assert.equal(script.messages.find((message) => message.id === 900).error.code, -32601);

    send(socket, { id: 901, method: "item/permissions/requestApproval", params: { threadId: "thread-A" } });
    await waitFor(() => script.messages.some((message) => message.id === 901));
    assert.equal(script.messages.find((message) => message.id === 901).error.code, -32601);

    send(socket, { method: "item/completed", params: { threadId: "thread-A", turnId: "turn-1", item: { type: "agentMessage", id: "m1", text: "All fixed" } } });
    send(socket, { method: "turn/completed", params: { threadId: "thread-A", turn: { id: "turn-1", status: "completed" } } });
    const inbox = path.join(home.dir, "thread-A.jsonl");
    await waitFor(() => inboxLines(inbox).length === 1);
    const lines = inboxLines(inbox);
    assert.equal(lines[0].kind, "completed");
    assert.equal(lines[0].status, "completed");
    assert.equal(lines[0].lastMessage, "All fixed");

    const listed = await mcp.call("list", {});
    assert.equal(listed.json.threads[0].unread, 1);
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
    assert.deepEqual(resume.params, { threadId: "thread-B" });
    // The turns a resume returns show a finished review as still running, so the latest turn is listed.
    const listing = script.messages.find((message) => message.method === "thread/turns/list");
    assert.deepEqual(listing.params, { threadId: "thread-B", limit: 1, sortDirection: "desc", itemsView: "full" });
    const lines = inboxLines(path.join(home.dir, "thread-B.jsonl"));
    assert.equal(lines.length, 1);
    assert.equal(lines[0].kind, "completed");
    assert.equal(lines[0].turnId, "turn-old");
    assert.equal(lines[0].lastMessage, "done while you were away");

    // A review that finished while disconnected is written from the resumed turn's items.
    const reviewOut = path.join(home.home, "late", "review.md");
    await writeFile(path.join(home.dir, "state.json"), JSON.stringify({ updatedAt: 2, threads: [{ id: "thread-R", name: "review", cwd: "/tmp", turnId: "turn-r", lastStatus: "inProgress", review: reviewOut }] }));
    script.resumeItems = [{ type: "exitedReviewMode", id: "x", review: "Review comment:\n\n- [P1] late finding — a.ts:1-2" }];
    const later = new McpChild(home.home, daemon.socketPath);
    try {
      await later.request("initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "test", version: "0" } });
      await later.call("list", {});
      assert.equal(await readFile(reviewOut, "utf8"), script.resumeItems[0].review);
      const late = inboxLines(path.join(home.dir, "thread-R.jsonl"));
      assert.equal(late[0].lastMessage, `Review written to ${reviewOut}`);
    } finally {
      await later.close();
    }
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

test("notify_claude and ask_claude reach the Claude session that supervises the calling thread", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  const daemon = await fakeDaemon(script.handler);
  const { mcp } = await startedThread(home, daemon, script);
  const tools = new ToolsChild(home.home);
  try {
    const init = await tools.request("initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "codex-mcp-client", version: "0" } });
    assert.equal(init.result.serverInfo.name, "claude-client");
    const listedTools = await tools.request("tools/list", {});
    assert.deepEqual(listedTools.result.tools.map((tool) => [tool.name, tool.inputSchema.required]), [["notify_claude", ["text"]], ["ask_claude", ["text"]]]);

    const inbox = path.join(home.dir, "thread-A.jsonl");
    const noted = await tools.tool("notify_claude", "thread-A", "call-1", "tests are red");
    assert.equal(noted.isError, false, noted.text);
    assert.equal(noted.text, "Delivered to Claude.");
    const [note] = inboxLines(inbox);
    assert.deepEqual({ ...note, ts: undefined }, { ts: undefined, kind: "notify", turnId: "turn-1", callId: "call-1", text: "tests are red" });

    let answered = false;
    const asking = tools.tool("ask_claude", "thread-A", "call-ask", "tests or implementation?").then((result) => { answered = true; return result; });
    await waitFor(() => inboxLines(inbox).some((line) => line.kind === "ask"));
    const ask = inboxLines(inbox).find((line) => line.kind === "ask");
    assert.deepEqual({ ...ask, ts: undefined }, { ts: undefined, kind: "ask", turnId: "turn-1", callId: "call-ask", text: "tests or implementation?" });
    const listed = await mcp.call("list", {});
    assert.deepEqual(listed.json.threads[0].waiting, [{ kind: "ask", callId: "call-ask", since: listed.json.threads[0].waiting[0].since, text: "tests or implementation?" }]);
    assert.equal(listed.json.attention.length, 1);
    assert.match(listed.json.attention[0], /thread-A/);
    const hook = await run(["pending"], { env: { CODEX_MANAGER_STATE_DIR: home.home }, stdin: JSON.stringify({ session_id: session, stop_hook_active: false }) });
    assert.match(JSON.parse(hook.stdout).reason, /thread-A is waiting for reply .*tests or implementation\?/);
    assert.equal(answered, false);

    const replied = await mcp.call("reply", { threadId: "thread-A", text: "Fix the implementation." });
    assert.equal(replied.isError, false, replied.text);
    assert.equal(replied.json.kind, "ask");
    const answer = await asking;
    assert.equal(answer.isError, false, answer.text);
    assert.equal(answer.text, "Fix the implementation.");
    const after = await mcp.call("list", {});
    assert.deepEqual(after.json.threads[0].waiting, []);
    assert.equal(after.json.attention, undefined);
    const late = await mcp.call("reply", { threadId: "thread-A", text: "again" });
    assert.equal(late.isError, true);
    assert.match(late.text, /not waiting/);
  } finally {
    await tools.close();
    await mcp.close();
    await daemon.close();
    await home.close();
  }
});

test("a thread nobody supervises, or whose Claude session is gone, is told so at once", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const tools = new ToolsChild(home.home);
  try {
    const stranger = await tools.tool("ask_claude", "thread-Z", "call-1", "anyone?");
    assert.equal(stranger.isError, true);
    assert.match(stranger.text, /No Claude session is supervising this thread/);
    const unnamed = await tools.tool("notify_claude", undefined, "call-2", "hello");
    assert.equal(unnamed.isError, true);
    assert.match(unnamed.text, /No Claude session is supervising this thread/);

    // A supervisor record whose process no longer runs.
    const gone = spawn(process.execPath, ["-e", ""]);
    await new Promise((resolve) => gone.on("close", resolve));
    await mkdir(path.join(home.home, "threads"), { recursive: true });
    await writeFile(path.join(home.home, "threads", "thread-Y.json"), JSON.stringify({ sessionId: session, pid: gone.pid }));
    const orphan = await tools.tool("ask_claude", "thread-Y", "call-3", "anyone?");
    assert.equal(orphan.isError, true);
    assert.match(orphan.text, /No Claude session is supervising this thread/);
    assert.deepEqual(inboxLines(path.join(home.dir, "thread-Y.jsonl")), []);
    const unknown = await tools.call("shout", { text: "x" }, { threadId: "thread-Y" });
    assert.equal(unknown.isError, true);
    assert.match(unknown.text, /unknown tool: shout/);
  } finally {
    await tools.close();
    await home.close();
  }
});

test("an unanswered ask times out with a proceed-on-your-own answer", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  const daemon = await fakeDaemon(script.handler);
  const { mcp } = await startedThread(home, daemon, script);
  const tools = new ToolsChild(home.home, { CODEX_MANAGER_ASK_TIMEOUT: "1" });
  try {
    const answer = await tools.tool("ask_claude", "thread-A", "call-slow", "?");
    assert.equal(answer.isError, false, answer.text);
    assert.match(answer.text, /did not answer within 1 seconds/);
    const listed = await mcp.call("list", {});
    assert.deepEqual(listed.json.threads[0].waiting, []);
  } finally {
    await tools.close();
    await mcp.close();
    await daemon.close();
    await home.close();
  }
});

test("a question is withdrawn when codex cancels the call or closes the server", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  const daemon = await fakeDaemon(script.handler);
  const { mcp } = await startedThread(home, daemon, script);
  const tools = new ToolsChild(home.home);
  const inbox = path.join(home.dir, "thread-A.jsonl");
  const waitingNow = async () => (await mcp.call("list", {})).json.threads[0].waiting.map((entry) => entry.callId);
  try {
    const asking = tools.tool("ask_claude", "thread-A", "call-1", "first?");
    await waitFor(() => inboxLines(inbox).length === 1);
    assert.deepEqual(await waitingNow(), ["call-1"]);
    tools.child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", method: "notifications/cancelled", params: { requestId: tools.nextId - 1 } })}\n`);
    const cancelled = await asking;
    assert.equal(cancelled.isError, true);
    assert.match(cancelled.text, /cancelled/);
    assert.deepEqual(await waitingNow(), []);

    tools.tool("ask_claude", "thread-A", "call-2", "second?");
    await waitFor(() => inboxLines(inbox).length === 2);
    assert.deepEqual(await waitingNow(), ["call-2"]);
    await tools.close();
    assert.deepEqual(await waitingNow(), []);
  } finally {
    await mcp.close();
    await daemon.close();
    await home.close();
  }
});

test("an attached session asks the Claude session that attached it", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  script.threads = sessionsOnDaemon();
  const daemon = await fakeDaemon(script.handler);
  const mcp = await mcpChild(home, daemon);
  const tools = new ToolsChild(home.home);
  try {
    await mcp.call("attach", { thread: "slow-requests" });
    const asking = tools.tool("ask_claude", "thread-5", "call-ask", "index or cache?");
    await waitFor(() => inboxLines(path.join(home.dir, "thread-5.jsonl")).some((line) => line.kind === "ask"));
    await mcp.call("reply", { threadId: "thread-5", text: "The index." });
    assert.equal((await asking).text, "The index.");
  } finally {
    await tools.close();
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
    const env = { CODEX_MANAGER_STATE_DIR: home.home };
    await mkdir(home.dir, { recursive: true });
    await writeFile(path.join(home.dir, "state.json"), JSON.stringify({ updatedAt: 1, threads: [{ id: "thread-C", lastStatus: "inProgress", waiting: [{ kind: "ask", callId: "c", since: 1, text: "which one?" }] }] }));
    await writeFile(path.join(home.dir, `thread-C.await.${process.pid}`), "");
    const first = await run(["pending"], { env, stdin: JSON.stringify({ session_id: session, stop_hook_active: false }) });
    assert.equal(first.code, 0, first.stderr);
    assert.match(JSON.parse(first.stdout).reason, /thread-C is waiting for reply/);
    const second = await run(["pending"], { env, stdin: JSON.stringify({ session_id: session, stop_hook_active: true }) });
    assert.equal(second.stdout, "");
  } finally {
    await home.close();
  }
});

test("after /clear gives Claude a new session id, await and pending still read the running server's events", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  const daemon = await fakeDaemon(script.handler);
  const mcp = new McpChild(home.home, daemon.socketPath);
  const cleared = "99999999-8888-7777-6666-555555555555";
  try {
    assert.ok(parentProcesses(mcp.child.pid).some(({ pid }) => pid === process.pid),
      "the /clear test needs permission to run ps and read the MCP server's parent process chain");
    await mcp.request("initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "test", version: "0" } });
    const started = await mcp.call("start", { cwd: "plugins", prompt: "Fix the bug" });
    assert.equal(started.isError, false, started.text);

    // Claude Code rewrites its session record on /clear; the server's own environment keeps the id it started with.
    await mkdir(path.join(home.home, "sessions"), { recursive: true });
    await writeFile(path.join(home.home, "sessions", `${process.pid}.json`), JSON.stringify({ pid: process.pid, sessionId: cleared }));
    const listed = await mcp.call("list", {});
    assert.equal(listed.isError, false, listed.text);

    const env = { CODEX_MANAGER_STATE_DIR: home.home, CLAUDE_CODE_SESSION_ID: cleared };
    await appendFile(path.join(home.dir, "thread-A.jsonl"), '{"kind":"completed","status":"completed"}\n');
    const awaited = await run(["await", "--thread", "thread-A", "--timeout", "3"], { env });
    assert.equal(awaited.code, 0, awaited.stderr);
    assert.match(awaited.stdout, /"kind":"completed"/);

    await appendFile(path.join(home.dir, "thread-A.jsonl"), '{"kind":"notify","text":"second"}\n');
    const stopped = await run(["pending"], { env, stdin: JSON.stringify({ session_id: cleared, stop_hook_active: false }) });
    assert.match(JSON.parse(stopped.stdout).reason, /second/);
  } finally {
    await mcp.close();
    await daemon.close();
    await home.close();
  }
});

test("pending blocks while a running thread has no await, however often Claude stops", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  try {
    const env = { CODEX_MANAGER_STATE_DIR: home.home };
    const stop = (active) => run(["pending"], { env, stdin: JSON.stringify({ session_id: session, stop_hook_active: active }) });
    const command = `node ${JSON.stringify(manager)} await --thread thread-R`;
    await mkdir(home.dir, { recursive: true });
    await writeFile(path.join(home.dir, "state.json"), JSON.stringify({ updatedAt: 1, threads: [{ id: "thread-R", lastStatus: "inProgress", waiting: [] }, { id: "thread-I", lastStatus: "completed", waiting: [] }] }));

    const unwatched = await stop(false);
    assert.equal(unwatched.code, 0, unwatched.stderr);
    const decision = JSON.parse(unwatched.stdout);
    assert.equal(decision.decision, "block");
    assert.ok(decision.reason.includes(command), decision.reason);
    assert.match(decision.reason, /run_in_background/);
    assert.doesNotMatch(decision.reason, /thread-I/);
    assert.ok(JSON.parse((await stop(true)).stdout).reason.includes(command));

    const waiting = run(["await", "--thread", "thread-R", "--timeout", "3"], { env });
    await waitFor(() => readdirSync(home.dir).some((name) => name.startsWith("thread-R.await.")));
    assert.equal((await stop(false)).stdout, "");
    assert.equal((await waiting).code, 124);
    assert.deepEqual(readdirSync(home.dir).filter((name) => name.startsWith("thread-R.await.")), []);
    assert.ok(JSON.parse((await stop(false)).stdout).reason.includes(command));

    const gone = spawn(process.execPath, ["-e", ""], { stdio: "ignore" });
    await new Promise((resolve) => gone.on("close", resolve));
    await writeFile(path.join(home.dir, `thread-R.await.${gone.pid}`), "");
    assert.ok(JSON.parse((await stop(false)).stdout).reason.includes(command));
    assert.deepEqual(readdirSync(home.dir).filter((name) => name.startsWith("thread-R.await.")), []);
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

test("several waiting requests are answered by callId, and a finished turn withdraws what is left", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  const daemon = await fakeDaemon(script.handler);
  const { mcp, socket } = await startedThread(home, daemon, script);
  const tools = new ToolsChild(home.home);
  try {
    const asking = tools.tool("ask_claude", "thread-A", "ask-1", "first?");
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

    send(socket, { method: "turn/completed", params: { threadId: "thread-A", turn: { id: "turn-1", status: "interrupted" } } });
    const withdrawn = await asking;
    assert.equal(withdrawn.isError, true);
    assert.match(withdrawn.text, /turn ended before Claude answered/);
    const listed = await mcp.call("list", {});
    assert.deepEqual(listed.json.threads[0].waiting, []);
    assert.equal(listed.json.attention, undefined);
  } finally {
    await tools.close();
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
      if (message.id !== undefined && message.method === "thread/turns/list") {
        script.messages.push(message);
        return send(socket, { id: message.id, result: { data: [{ id: "turn-1", status: "completed", items: [] }] } });
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

test("review starts a read-only thread, runs codex's review mode on a base sha, and saves the review text", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  const daemon = await fakeDaemon(script.handler);
  const mcp = new McpChild(home.home, daemon.socketPath);
  const out = path.join(home.home, "specs", "review", "review.md");
  try {
    const review = await mcp.call("review", { cwd: "plugins", base: "abc1234", plan: "specs/plan.md", out, name: "review-feature" });
    assert.equal(review.isError, false, review.text);
    assert.equal(review.json.threadId, "thread-A");
    assert.equal(review.json.turnId, "turn-review");
    assert.equal(review.json.out, out);
    assert.equal(review.json.await, `node ${JSON.stringify(manager)} await --thread thread-A`);

    const methods = script.messages.map((message) => message.method);
    assert.deepEqual(methods, ["initialize", "initialized", "thread/start", "thread/name/set", "review/start"]);
    const start = script.messages[2].params;
    assert.equal(start.cwd, path.resolve("plugins"));
    assert.equal(start.sandbox, "read-only");
    assert.equal(start.dynamicTools, undefined);
    const started = script.messages[4].params;
    assert.equal(started.threadId, "thread-A");
    assert.equal(started.delivery, "inline");
    assert.equal(started.target.type, "custom");
    assert.match(started.target.instructions, /since commit abc1234/);
    assert.match(started.target.instructions, /git diff abc1234/);
    assert.match(started.target.instructions, /specs\/plan\.md/);
    assert.match(started.target.instructions, /Provide prioritized, actionable findings\./);

    const [socket] = script.sockets;
    const text = "The patch is correct.\n\nReview comment:\n\n- [P2] Guard the empty list — src/a.ts:10-12\n  The loop assumes one element.";
    send(socket, { method: "item/completed", params: { threadId: "thread-A", turnId: "turn-review", item: { type: "exitedReviewMode", id: "r1", review: text } } });
    send(socket, { method: "item/completed", params: { threadId: "thread-A", turnId: "turn-review", item: { type: "agentMessage", id: "m9", text } } });
    // The daemon's finished turn carries the agent message, which repeats the review, and not the review item.
    send(socket, { method: "turn/completed", params: { threadId: "thread-A", turn: { id: "turn-review", status: "completed", items: [{ type: "agentMessage", id: "m9", text }] } } });
    const inbox = path.join(home.dir, "thread-A.jsonl");
    await waitFor(() => inboxLines(inbox).length === 1);
    assert.equal(await readFile(out, "utf8"), text);
    const [done] = inboxLines(inbox);
    assert.equal(done.kind, "completed");
    assert.equal(done.status, "completed");
    assert.equal(done.lastMessage, `Review written to ${out}`);
    const listed = await mcp.call("list", {});
    assert.equal(listed.json.threads[0].review, out);
  } finally {
    await mcp.close();
    await daemon.close();
    await home.close();
  }
});

test("review takes an adversarial stance and a focus, and refuses a stance it does not know", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  const daemon = await fakeDaemon(script.handler);
  const mcp = new McpChild(home.home, daemon.socketPath);
  const out = path.join(home.home, "specs", "review", "adversarial.md");
  try {
    const refused = await mcp.call("review", { cwd: "plugins", base: "abc1234", out, stance: "friendly" });
    assert.equal(refused.isError, true);
    assert.match(refused.text, /stance must be "adversarial"/);
    assert.equal(script.messages.some((message) => message.method === "review/start"), false);

    const review = await mcp.call("review", { cwd: "plugins", base: "abc1234", plan: "specs/plan.md", out, stance: "adversarial", focus: "the retry path of the upload queue" });
    assert.equal(review.isError, false, review.text);
    const started = script.messages.find((message) => message.method === "review/start").params;
    assert.equal(started.target.type, "custom");
    assert.match(started.target.instructions, /^Review the code changes since commit abc1234\./);
    assert.match(started.target.instructions, /specs\/plan\.md/);
    assert.match(started.target.instructions, /strongest reasons this change should not ship/);
    assert.match(started.target.instructions, /Focus: the retry path of the upload queue/);
    assert.match(started.target.instructions, /Provide prioritized, actionable findings\.$/);
  } finally {
    await mcp.close();
    await daemon.close();
    await home.close();
  }
});

test("review without a stance carries a focus and no adversarial wording", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  const daemon = await fakeDaemon(script.handler);
  const mcp = new McpChild(home.home, daemon.socketPath);
  const out = path.join(home.home, "specs", "review", "review.md");
  try {
    const review = await mcp.call("review", { cwd: "plugins", base: "abc1234", out, focus: "the migration" });
    assert.equal(review.isError, false, review.text);
    const started = script.messages.find((message) => message.method === "review/start").params;
    assert.match(started.target.instructions, /Focus: the migration/);
    assert.doesNotMatch(started.target.instructions, /should not ship/);
  } finally {
    await mcp.close();
    await daemon.close();
    await home.close();
  }
});

test("review points the reviewer at the decisions made during the run", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  const daemon = await fakeDaemon(script.handler);
  const mcp = new McpChild(home.home, daemon.socketPath);
  const out = path.join(home.home, "specs", "review", "review.md");
  try {
    const review = await mcp.call("review", { cwd: "plugins", base: "abc1234", plan: "specs/plan.md", decisions: "specs/decisions.md", out });
    assert.equal(review.isError, false, review.text);
    const started = script.messages.find((message) => message.method === "review/start").params;
    assert.match(started.target.instructions, /specs\/plan\.md/);
    assert.match(started.target.instructions, /specs\/decisions\.md/);
    assert.match(started.target.instructions, /do not flag one as a departure from the spec/);
    assert.match(started.target.instructions, /judge each rule in it on its own/);
    assert.match(started.target.instructions, /wrong result for a caller or a user/);
    assert.match(started.target.instructions, /in code the changes did not touch/);
    assert.match(started.target.instructions, /Provide prioritized, actionable findings\.$/);

    const plain = await mcp.call("review", { cwd: "plugins", base: "abc1234", out });
    assert.equal(plain.isError, false, plain.text);
    const last = script.messages.filter((message) => message.method === "review/start").at(-1).params;
    assert.doesNotMatch(last.target.instructions, /decisions/);
  } finally {
    await mcp.close();
    await daemon.close();
    await home.close();
  }
});

const sessionsOnDaemon = () => [
  { id: "thread-1", name: "fix-login", cwd: "/repo/a", status: { type: "idle" } },
  { id: "thread-2", name: "twin", cwd: "/repo/b", status: { type: "active", activeFlags: [] } },
  { id: "thread-3", name: null, cwd: "/repo/c", status: { type: "notLoaded" } },
  { id: "thread-4", name: "twin", cwd: "/repo/d", status: { type: "notLoaded" } },
  { id: "thread-5", name: "slow-requests", cwd: "/repo/e", status: { type: "active", activeFlags: [] } },
  { id: "thread-6", name: "stale", cwd: "/repo/f", status: { type: "notLoaded" } },
  { id: "thread-7", name: "stale", cwd: "/repo/g", status: { type: "notLoaded" } }
];

async function mcpChild(home, daemon, env) {
  const mcp = new McpChild(home.home, daemon.socketPath, env);
  await mcp.request("initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "test", version: "0" } });
  return mcp;
}

test("attach finds a session by name across pages, subscribes without changing its settings, and wakes on its completion", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  script.threads = sessionsOnDaemon();
  script.resumeTurns = [{ id: "turn-live", status: "inProgress", items: [] }];
  script.activeTurn = "turn-live";
  const daemon = await fakeDaemon(script.handler);
  const mcp = await mcpChild(home, daemon);
  try {
    const attached = await mcp.call("attach", { thread: "slow-requests" });
    assert.equal(attached.isError, false, attached.text);
    assert.deepEqual(attached.json, {
      threadId: "thread-5",
      name: "slow-requests",
      cwd: "/repo/e",
      attached: true,
      lastStatus: "inProgress",
      await: `node ${JSON.stringify(manager)} await --thread thread-5`,
      note: attached.json.note
    });
    assert.match(attached.json.note, /Approvals and questions stay with the client/);

    const lists = script.messages.filter((message) => message.method === "thread/list");
    assert.deepEqual(lists.map((message) => message.params.cursor), [undefined, "2", "4", "6"]);
    assert.equal(lists[0].params.searchTerm, undefined);
    const resumes = script.messages.filter((message) => message.method === "thread/resume");
    assert.deepEqual(resumes.map((message) => message.params), [{ threadId: "thread-5" }]);
    assert.equal(script.messages.some((message) => message.method === "thread/start"), false);

    const state = JSON.parse(await readFile(path.join(home.dir, "state.json"), "utf8"));
    assert.deepEqual(state.threads, [{ id: "thread-5", name: "slow-requests", cwd: "/repo/e", turnId: "turn-live", lastStatus: "inProgress", waiting: [], attached: true }]);

    const sent = await mcp.call("send", { threadId: "thread-5", prompt: "steer: use the index instead" });
    assert.equal(sent.isError, false, sent.text);
    assert.equal(script.messages.at(-1).params.threadId, "thread-5");
    assert.equal(sent.json.steered, true);
    const interrupted = await mcp.call("interrupt", { threadId: "thread-5" });
    assert.equal(interrupted.isError, false, interrupted.text);
    assert.deepEqual(script.messages.at(-1).params, { threadId: "thread-5", turnId: "turn-live" });

    const [socket] = script.sockets;
    const turnId = sent.json.turnId;
    send(socket, { method: "item/completed", params: { threadId: "thread-5", turnId, item: { type: "agentMessage", id: "m1", text: "Switched to the index" } } });
    send(socket, { method: "turn/completed", params: { threadId: "thread-5", turn: { id: turnId, status: "completed" } } });
    const inbox = path.join(home.dir, "thread-5.jsonl");
    await waitFor(() => inboxLines(inbox).length === 1);
    const [done] = inboxLines(inbox);
    assert.equal(done.kind, "completed");
    assert.equal(done.lastMessage, "Switched to the index");
    const listed = await mcp.call("list", {});
    assert.equal(listed.json.threads[0].attached, true);
    assert.equal(listed.json.threads[0].lastStatus, "completed");
    assert.equal(listed.json.threads[0].unread, 1);
  } finally {
    await mcp.close();
    await daemon.close();
    await home.close();
  }
});

test("attach takes a thread id, reports a finished session without an inbox event, and is repeatable", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  script.threads = sessionsOnDaemon();
  const daemon = await fakeDaemon(script.handler);
  const mcp = await mcpChild(home, daemon);
  try {
    const attached = await mcp.call("attach", { thread: "thread-4" });
    assert.equal(attached.isError, false, attached.text);
    assert.equal(attached.json.threadId, "thread-4");
    assert.equal(attached.json.name, "twin");
    assert.equal(attached.json.lastStatus, "completed");
    assert.deepEqual(inboxLines(path.join(home.dir, "thread-4.jsonl")), []);

    const again = await mcp.call("attach", { thread: "thread-4" });
    assert.equal(again.isError, false, again.text);
    assert.equal(again.json.threadId, "thread-4");
    assert.equal(script.messages.filter((message) => message.method === "thread/resume").length, 1);
    const listed = await mcp.call("list", {});
    assert.equal(listed.json.threads.length, 1);

    script.resumeTurns = [];
    const untouched = await mcp.call("attach", { thread: "fix-login" });
    assert.equal(untouched.json.lastStatus, "idle");
    const noTurn = await mcp.call("interrupt", { threadId: "thread-1" });
    assert.match(noTurn.text, /no turn to interrupt/);
  } finally {
    await mcp.close();
    await daemon.close();
    await home.close();
  }
});

test("attach refuses a name no session has, and a name several sessions share", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  script.threads = sessionsOnDaemon();
  const daemon = await fakeDaemon(script.handler);
  const mcp = await mcpChild(home, daemon);
  try {
    const missing = await mcp.call("attach", { thread: "fix-log" });
    assert.equal(missing.isError, true);
    assert.match(missing.text, /no codex session has the id or name "fix-log"/);
    const shared = await mcp.call("attach", { thread: "stale" });
    assert.equal(shared.isError, true);
    assert.match(shared.text, /2 codex sessions are named "stale"/);
    assert.match(shared.text, /thread-6 \(notLoaded, \/repo\/f\)/);
    assert.match(shared.text, /thread-7 \(notLoaded, \/repo\/g\)/);
    const unnamed = await mcp.call("attach", {});
    assert.equal(unnamed.isError, true);
    assert.match(unnamed.text, /thread is required/);
    assert.equal(script.messages.some((message) => message.method === "thread/resume"), false);
    assert.deepEqual((await mcp.call("list", {})).json.threads, []);
  } finally {
    await mcp.close();
    await daemon.close();
    await home.close();
  }
});

test("attach takes the one session that is open when others share its name", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  script.threads = sessionsOnDaemon();
  const daemon = await fakeDaemon(script.handler);
  const mcp = await mcpChild(home, daemon);
  try {
    const attached = await mcp.call("attach", { thread: "twin" });
    assert.equal(attached.isError, false, attached.text);
    assert.equal(attached.json.threadId, "thread-2");

    script.threads[3].status = { type: "idle" };
    const bothOpen = await mcp.call("attach", { thread: "twin" });
    assert.equal(bothOpen.isError, true);
    assert.match(bothOpen.text, /2 codex sessions are named "twin"/);
    assert.match(bothOpen.text, /thread-2 \(active, \/repo\/b\)/);
    assert.match(bothOpen.text, /thread-4 \(idle, \/repo\/d\)/);
  } finally {
    await mcp.close();
    await daemon.close();
    await home.close();
  }
});

test("attach records nothing when the daemon cannot resume the session", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  script.threads = sessionsOnDaemon();
  const refusing = (message, socket) => {
    if (message.method === "thread/resume") return send(socket, { id: message.id, error: { code: -32000, message: "no rollout found" } });
    script.handler(message, socket);
  };
  const daemon = await fakeDaemon(refusing);
  const mcp = await mcpChild(home, daemon);
  try {
    const attached = await mcp.call("attach", { thread: "fix-login" });
    assert.equal(attached.isError, true);
    assert.match(attached.text, /no rollout found/);
    assert.deepEqual((await mcp.call("list", {})).json.threads, []);
  } finally {
    await mcp.close();
    await daemon.close();
    await home.close();
  }
});

test("requests the daemon sends for an attached session are left to the client it runs in, also after a restart", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  script.threads = sessionsOnDaemon();
  script.resumeTurns = [{ id: "turn-live", status: "inProgress", items: [] }];
  const daemon = await fakeDaemon(script.handler);
  const env = { CODEX_MANAGER_ASK_TIMEOUT: "1" };
  const mcp = await mcpChild(home, daemon, env);
  let restarted = false;
  const inbox = path.join(home.dir, "thread-5.jsonl");
  const silentOn = async (socket, firstId) => {
    send(socket, { id: firstId, method: "item/commandExecution/requestApproval", params: { threadId: "thread-5", turnId: "turn-live", itemId: "item-cmd", command: "rm -rf build" } });
    send(socket, { id: firstId + 1, method: "item/fileChange/requestApproval", params: { threadId: "thread-5", turnId: "turn-live", itemId: "item-fc" } });
    send(socket, { id: firstId + 2, method: "item/tool/call", params: { threadId: "thread-5", turnId: "turn-live", callId: "call-1", tool: "create_thread", arguments: {} } });
    // Longer than the ask timeout, so a held request would have been answered by now.
    await new Promise((resolve) => setTimeout(resolve, 1600));
    assert.deepEqual(script.messages.filter((message) => message.id >= firstId && message.method === undefined), []);
    assert.deepEqual(inboxLines(inbox), []);
  };
  try {
    await mcp.call("attach", { thread: "slow-requests" });
    const [socket] = script.sockets;
    await silentOn(socket, 970);
    const listed = await mcp.call("list", {});
    assert.deepEqual(listed.json.threads[0].waiting, []);
    assert.equal(listed.json.attention, undefined);
    await mcp.close();
    restarted = true;

    const later = await mcpChild(home, daemon, env);
    try {
      const adopted = await later.call("list", {});
      assert.equal(adopted.json.threads[0].attached, true);
      assert.equal(script.messages.filter((message) => message.method === "thread/resume").length, 2);
      await waitFor(() => script.sockets.size === 2);
      await silentOn([...script.sockets][1], 980);
    } finally {
      await later.close();
    }
  } finally {
    // A failure before the restart would leave the first manager running and the test runner waiting on it.
    if (!restarted) await mcp.close();
    await daemon.close();
    await home.close();
  }
});

test("detach unsubscribes a finished thread, refuses a running one, and send picks the thread up again", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  const script = daemonScript();
  const daemon = await fakeDaemon(script.handler);
  const { mcp, socket } = await startedThread(home, daemon, script);
  try {
    const running = await mcp.call("detach", { threadId: "thread-A" });
    assert.equal(running.isError, true);
    assert.match(running.text, /still running/);
    assert.equal(script.messages.some((message) => message.method === "thread/unsubscribe"), false);

    send(socket, { method: "turn/completed", params: { threadId: "thread-A", turn: { id: "turn-1", status: "completed" } } });
    await waitFor(() => inboxLines(path.join(home.dir, "thread-A.jsonl")).length === 1);
    const detached = await mcp.call("detach", { threadId: "thread-A" });
    assert.equal(detached.isError, false, detached.text);
    assert.deepEqual(detached.json, { threadId: "thread-A", detached: true, status: "unsubscribed" });
    assert.deepEqual(script.messages.at(-1).method, "thread/unsubscribe");
    assert.deepEqual(script.messages.at(-1).params, { threadId: "thread-A" });
    const listed = await mcp.call("list", {});
    assert.equal(listed.json.threads[0].detached, true);
    assert.equal(listed.json.threads[0].unread, 1);

    const unknown = await mcp.call("detach", { threadId: "nope" });
    assert.equal(unknown.isError, true);
    assert.match(unknown.text, /unknown thread/);

    script.threads = [{ id: "thread-A", name: null, cwd: "/repo", status: { type: "notLoaded" } }];
    const reattached = await mcp.call("attach", { thread: "thread-A" });
    assert.equal(reattached.isError, false, reattached.text);
    assert.equal(reattached.json.attached, false);
    assert.deepEqual(script.messages.slice(-2).map((message) => message.method), ["thread/resume", "thread/turns/list"]);
    assert.equal((await mcp.call("list", {})).json.threads[0].detached, undefined);
    await mcp.call("detach", { threadId: "thread-A" });

    const sent = await mcp.call("send", { threadId: "thread-A", prompt: "one more thing" });
    assert.equal(sent.isError, false, sent.text);
    assert.deepEqual(script.messages.slice(-3).map((message) => message.method), ["thread/resume", "thread/turns/list", "turn/start"]);
    const state = JSON.parse(await readFile(path.join(home.dir, "state.json"), "utf8"));
    assert.equal(state.threads[0].detached, undefined);
    assert.equal(state.threads[0].lastStatus, "inProgress");
  } finally {
    await mcp.close();
    await daemon.close();
    await home.close();
  }
});

test("a detached thread is not subscribed to again when the server starts or reconnects", { timeout: 20_000 }, async () => {
  const home = await tempHome();
  await mkdir(home.dir, { recursive: true });
  await writeFile(path.join(home.dir, "state.json"), JSON.stringify({ updatedAt: 1, threads: [
    { id: "thread-done", name: "done", cwd: "/tmp", turnId: "turn-old", lastStatus: "completed", detached: true },
    { id: "thread-live", name: "live", cwd: "/tmp", turnId: "turn-old", lastStatus: "completed" }
  ] }));
  const script = daemonScript();
  const daemon = await fakeDaemon(script.handler);
  const mcp = await mcpChild(home, daemon);
  try {
    await mcp.call("list", {});
    const resumed = () => script.messages.filter((message) => message.method === "thread/resume").map((message) => message.params.threadId);
    assert.deepEqual(resumed(), ["thread-live"]);
    const [socket] = script.sockets;
    socket.destroy();
    await waitFor(() => resumed().length === 2, { timeout: 8000 });
    assert.deepEqual(resumed(), ["thread-live", "thread-live"]);

    // With nothing left to listen to, a starting server leaves the daemon alone.
    await writeFile(path.join(home.dir, "state.json"), JSON.stringify({ updatedAt: 2, threads: [{ id: "thread-done", name: "done", cwd: "/tmp", turnId: "turn-old", lastStatus: "completed", detached: true }] }));
    const before = script.messages.length;
    const idle = await mcpChild(home, daemon);
    try {
      await new Promise((resolve) => setTimeout(resolve, 300));
      assert.equal(script.messages.length, before);
    } finally {
      await idle.close();
    }
  } finally {
    await mcp.close();
    await daemon.close();
    await home.close();
  }
});
