#!/usr/bin/env node
// ABOUTME: Lets Claude Code run codex threads on the shared app-server daemon as supervised workers.
// ABOUTME: `mcp` serves Claude's tools and relays codex events; `await` and `pending` deliver them.

import { readFile } from "node:fs/promises";
import path from "node:path";
import readline from "node:readline";
import { fileURLToPath, pathToFileURL } from "node:url";
import { WebSocketClient, daemonSocketPath } from "../lib/daemon-client.mjs";
import { SessionStore } from "./inbox.mjs";
import { resolveSessionId } from "./session.mjs";

const scriptPath = fileURLToPath(import.meta.url);
const usage = `usage:
  codex-manager.mjs mcp                                   serve Claude's codex tools over stdio
  codex-manager.mjs await --thread <id> [--timeout <s>]   print the next inbox events and exit
  codex-manager.mjs pending                               Stop hook: block on undelivered events
  codex-manager.mjs whoami                                print the resolved Claude session id`;

const NOTIFY_TOOL = {
  type: "function",
  name: "notify_claude",
  description: "Send a short progress note or question to Claude, the supervisor who started this thread. Claude reads it asynchronously; keep working after calling it unless you need a decision, in which case finish this turn after sending.",
  inputSchema: { type: "object", properties: { text: { type: "string", description: "The message for Claude." } }, required: ["text"], additionalProperties: false }
};

function log(message) {
  process.stderr.write(`codex-manager: ${message}\n`);
}

function parseArgs(argv) {
  const options = {};
  const positional = [];
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--thread" || argument === "--timeout") {
      if (index + 1 >= argv.length) throw new Error(`missing value for ${argument}`);
      options[argument.slice(2)] = argv[++index];
    } else if (argument.startsWith("-")) {
      throw new Error(`unknown option: ${argument}`);
    } else {
      positional.push(argument);
    }
  }
  return { command: positional[0], options };
}

function awaitCommand(threadId) {
  return `node ${scriptPath} await --thread ${threadId}`;
}

// --- await -------------------------------------------------------------------

async function runAwait(options) {
  if (!options.thread) throw new Error("--thread is required");
  const store = new SessionStore(await resolveSessionId());
  const timeoutMs = options.timeout === undefined ? undefined : Number(options.timeout) * 1000;
  if (timeoutMs !== undefined && !(timeoutMs > 0)) throw new Error("--timeout must be a positive number of seconds");
  const started = Date.now();
  for (;;) {
    const { lines, end } = store.unread(options.thread);
    if (lines.length) {
      store.advance(options.thread, end);
      process.stdout.write(`${lines.join("\n")}\n`);
      return 0;
    }
    if (timeoutMs !== undefined && Date.now() - started >= timeoutMs) return 124;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
}

// --- pending (Stop hook) -----------------------------------------------------

async function runPending() {
  const input = await new Promise((resolve) => {
    let text = "";
    process.stdin.setEncoding("utf8");
    process.stdin.on("data", (chunk) => { text += chunk; });
    process.stdin.on("end", () => resolve(text));
  });
  let sessionId;
  try {
    sessionId = JSON.parse(input).session_id;
  } catch {
    return 0;
  }
  if (!sessionId) return 0;
  const store = new SessionStore(sessionId);
  if (!store.exists()) return 0;
  const sections = [];
  for (const threadId of store.threadIds()) {
    const { lines, end } = store.unread(threadId);
    if (!lines.length) continue;
    store.advance(threadId, end);
    sections.push(`codex thread ${threadId}:\n${lines.join("\n")}`);
  }
  if (!sections.length) return 0;
  const reason = `codex-manager delivered ${sections.length} thread(s) with undelivered events. Handle them before stopping.\n\n${sections.join("\n\n")}`;
  process.stdout.write(`${JSON.stringify({ decision: "block", reason })}\n`);
  return 0;
}

// --- mcp ---------------------------------------------------------------------

class Manager {
  constructor(store, socketPath, version) {
    this.store = store;
    this.socketPath = socketPath;
    this.version = version;
    this.client = undefined;
    this.connecting = undefined;
    this.lastMessages = new Map();
  }

  state() {
    return this.store.readState();
  }

  thread(threadId) {
    const found = this.state().threads.find((thread) => thread.id === threadId);
    if (!found) throw new Error(`unknown thread: ${threadId}`);
    return found;
  }

  updateThread(threadId, patch) {
    const state = this.state();
    const index = state.threads.findIndex((thread) => thread.id === threadId);
    if (index === -1) state.threads.push({ id: threadId, ...patch });
    else state.threads[index] = { ...state.threads[index], ...patch };
    this.store.writeState(state);
  }

  connected() {
    return Boolean(this.client);
  }

  async connect() {
    if (this.client) return this.client;
    if (this.connecting) return this.connecting;
    this.connecting = this.connectInner().finally(() => { this.connecting = undefined; });
    return this.connecting;
  }

  async connectInner() {
    const client = new WebSocketClient(this.socketPath);
    client.onNotification = (message) => this.onNotification(message);
    client.onServerRequest = (message) => this.onServerRequest(client, message);
    client.onFailure = (error) => {
      log(`daemon connection lost: ${error.message}`);
      if (this.client === client) this.client = undefined;
    };
    await client.connect();
    await client.request("initialize", {
      clientInfo: { title: "codex-manager", name: "codex-manager", version: this.version },
      capabilities: {
        experimentalApi: true,
        requestAttestation: false,
        optOutNotificationMethods: ["item/agentMessage/delta", "item/reasoning/summaryTextDelta", "item/reasoning/summaryPartAdded", "item/reasoning/textDelta"]
      }
    });
    client.send({ method: "initialized", params: {} });
    this.client = client;
    await this.adopt(client);
    return client;
  }

  /** Re-subscribes to every recorded thread and backfills turns that ended while disconnected. */
  async adopt(client) {
    for (const thread of this.state().threads) {
      let resumed;
      try {
        resumed = await client.request("thread/resume", { threadId: thread.id, initialTurnsPage: { limit: 1, sortDirection: "desc" } });
      } catch (error) {
        log(`cannot resume thread ${thread.id}: ${error.message}`);
        this.updateThread(thread.id, { lastStatus: "unavailable", error: error.message });
        continue;
      }
      const latest = resumed.initialTurnsPage?.data?.[0];
      if (!latest) continue;
      const finished = latest.status !== "inProgress";
      if (finished && thread.lastStatus === "inProgress") {
        this.recordCompletion(thread.id, latest);
      } else if (latest.id !== thread.turnId || latest.status !== thread.lastStatus) {
        this.updateThread(thread.id, { turnId: latest.id, lastStatus: latest.status });
      }
    }
  }

  recordCompletion(threadId, turn) {
    const fromItems = turn.items?.filter((item) => item.type === "agentMessage").at(-1)?.text;
    const lastMessage = fromItems ?? this.lastMessages.get(threadId) ?? "";
    this.lastMessages.delete(threadId);
    this.store.append(threadId, { kind: "completed", turnId: turn.id, status: turn.status, lastMessage, error: turn.error?.message });
    this.updateThread(threadId, { turnId: turn.id, lastStatus: turn.status });
  }

  onNotification({ method, params = {} }) {
    const threadId = params.threadId;
    if (!threadId || !this.state().threads.some((thread) => thread.id === threadId)) return;
    if (method === "item/completed" && params.item?.type === "agentMessage") {
      this.lastMessages.set(threadId, params.item.text || "");
    } else if (method === "turn/started") {
      this.updateThread(threadId, { turnId: params.turn?.id, lastStatus: "inProgress" });
    } else if (method === "turn/completed") {
      this.recordCompletion(threadId, params.turn);
    }
  }

  onServerRequest(client, message) {
    const { id, method, params = {} } = message;
    if (method === "item/tool/call" && params.tool === NOTIFY_TOOL.name) {
      this.store.append(params.threadId, { kind: "notify", turnId: params.turnId, callId: params.callId, text: params.arguments?.text ?? "" });
      client.send({ id, result: { contentItems: [{ type: "inputText", text: "Delivered to Claude." }], success: true } });
      return;
    }
    client.send({ id, error: { code: -32601, message: `codex-manager does not handle ${method}` } });
  }

  async start({ cwd, prompt, name }) {
    if (!cwd || !prompt) throw new Error("cwd and prompt are required");
    const client = await this.connect();
    const absolute = path.resolve(cwd);
    const started = await client.request("thread/start", {
      cwd: absolute,
      approvalPolicy: "never",
      sandbox: "workspace-write",
      config: { "sandbox_workspace_write.network_access": true },
      serviceName: "codex-manager",
      ephemeral: false,
      dynamicTools: [NOTIFY_TOOL]
    });
    const threadId = started.thread.id;
    if (name) await client.request("thread/name/set", { threadId, name });
    this.updateThread(threadId, { name: name ?? null, cwd: absolute, turnId: null, lastStatus: "inProgress" });
    const turn = await this.startTurn(client, threadId, prompt);
    return { threadId, turnId: turn.id, name: name ?? null, cwd: absolute, await: awaitCommand(threadId), note: "Run the await command with run_in_background; it exits when codex finishes the turn or calls notify_claude." };
  }

  async startTurn(client, threadId, prompt) {
    const response = await client.request("turn/start", { threadId, input: [{ type: "text", text: prompt, text_elements: [] }] });
    this.updateThread(threadId, { turnId: response.turn.id, lastStatus: "inProgress" });
    return response.turn;
  }

  async send({ threadId, prompt }) {
    if (!threadId || !prompt) throw new Error("threadId and prompt are required");
    const thread = this.thread(threadId);
    const client = await this.connect();
    const wasRunning = thread.lastStatus === "inProgress";
    const turn = await this.startTurn(client, threadId, prompt);
    return { threadId, turnId: turn.id, steered: wasRunning, await: awaitCommand(threadId), note: wasRunning ? "A turn was already running, so the prompt was injected into it." : "Run the await command with run_in_background." };
  }

  async interrupt({ threadId }) {
    if (!threadId) throw new Error("threadId is required");
    const thread = this.thread(threadId);
    if (!thread.turnId) throw new Error(`thread ${threadId} has no turn to interrupt`);
    const client = await this.connect();
    await client.request("turn/interrupt", { threadId, turnId: thread.turnId });
    return { threadId, turnId: thread.turnId, interrupted: true };
  }

  async list() {
    if (!this.client) await this.connect().catch((error) => log(`not connected: ${error.message}`));
    const threads = this.state().threads.map((thread) => ({ ...thread, unread: this.store.unreadCount(thread.id), await: awaitCommand(thread.id) }));
    return { connected: this.connected(), sessionDir: this.store.dir, threads };
  }
}

const TOOLS = [
  { name: "start", description: "Start a codex worker thread on the shared daemon and give it a task. Returns the thread id and the await command to run in the background.", inputSchema: { type: "object", properties: { cwd: { type: "string", description: "Absolute working directory for the worker." }, prompt: { type: "string", description: "The task, written for a worker that sees nothing of this conversation." }, name: { type: "string", description: "Short human-readable thread name." } }, required: ["cwd", "prompt"] } },
  { name: "send", description: "Send a follow-up prompt to a codex thread. Starts a new turn when idle; when a turn is running the prompt is injected into it.", inputSchema: { type: "object", properties: { threadId: { type: "string" }, prompt: { type: "string" } }, required: ["threadId", "prompt"] } },
  { name: "interrupt", description: "Interrupt the running turn of a codex thread.", inputSchema: { type: "object", properties: { threadId: { type: "string" } }, required: ["threadId"] } },
  { name: "list", description: "List the codex threads this session started, their last turn status, and how many inbox events are still unread.", inputSchema: { type: "object", properties: {} } }
];

async function runMcp() {
  const plugin = JSON.parse(await readFile(path.resolve(path.dirname(scriptPath), "../.claude-plugin/plugin.json"), "utf8"));
  const sessionId = await resolveSessionId();
  const store = new SessionStore(sessionId);
  const manager = new Manager(store, daemonSocketPath(process.env.CODEX_MANAGER_DAEMON_SOCKET), plugin.version);
  log(`session ${sessionId}, state in ${store.dir}`);
  if (store.readState().threads.length) manager.connect().catch((error) => log(`adoption deferred: ${error.message}`));

  const reply = (id, body) => process.stdout.write(`${JSON.stringify({ jsonrpc: "2.0", id, ...body })}\n`);
  const handle = async (message) => {
    const { id, method, params = {} } = message;
    if (method === "initialize") return reply(id, { result: { protocolVersion: params.protocolVersion || "2025-06-18", capabilities: { tools: {} }, serverInfo: { name: "codex-manager", version: plugin.version } } });
    if (method === "ping") return reply(id, { result: {} });
    if (method === "tools/list") return reply(id, { result: { tools: TOOLS } });
    if (method === "tools/call") {
      const tool = TOOLS.find((candidate) => candidate.name === params.name);
      if (!tool) return reply(id, { error: { code: -32602, message: `unknown tool: ${params.name}` } });
      try {
        const result = await manager[tool.name](params.arguments ?? {});
        return reply(id, { result: { content: [{ type: "text", text: JSON.stringify(result, null, 2) }] } });
      } catch (error) {
        return reply(id, { result: { content: [{ type: "text", text: error.message }], isError: true } });
      }
    }
    if (id === undefined) return undefined;
    return reply(id, { error: { code: -32601, message: `method not found: ${method}` } });
  };

  const inflight = new Set();
  const lines = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
  for await (const line of lines) {
    if (!line.trim()) continue;
    let message;
    try {
      message = JSON.parse(line);
    } catch {
      log(`ignoring invalid JSON: ${line.slice(0, 80)}`);
      continue;
    }
    const work = handle(message).catch((error) => log(`unhandled: ${error.stack || error.message}`));
    inflight.add(work);
    work.finally(() => inflight.delete(work));
  }
  await Promise.allSettled(inflight);
  manager.client?.close();
  return 0;
}

// --- entry -------------------------------------------------------------------

export async function main(argv) {
  const { command, options } = parseArgs(argv);
  if (command === "mcp") return runMcp();
  if (command === "await") return runAwait(options);
  if (command === "pending") return runPending();
  if (command === "whoami") {
    process.stdout.write(`${await resolveSessionId()}\n`);
    return 0;
  }
  throw new Error(command ? `unknown command: ${command}` : "missing command");
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    process.exitCode = await main(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`${error.message}\n${usage}\n`);
    process.exitCode = error.exitCode || 2;
  }
}
