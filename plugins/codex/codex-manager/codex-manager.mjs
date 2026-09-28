#!/usr/bin/env node
// ABOUTME: Lets Claude Code run codex threads on the shared app-server daemon as supervised workers.
// ABOUTME: `mcp` serves Claude's tools and relays codex events; `await` and `pending` deliver them.

import { readFile } from "node:fs/promises";
import path from "node:path";
import readline from "node:readline";
import { fileURLToPath, pathToFileURL } from "node:url";
import { WebSocketClient, daemonSocketPath } from "../lib/daemon-client.mjs";
import { readStdin } from "../lib/stdin.mjs";
import { SessionStore } from "./inbox.mjs";
import { resolveSessionId } from "./session.mjs";

const scriptPath = fileURLToPath(import.meta.url);
const usage = `usage:
  codex-manager.mjs mcp                                   serve Claude's codex tools over stdio
  codex-manager.mjs await --thread <id> [--timeout <s>]   print the next inbox events and exit
  codex-manager.mjs pending                               Stop hook: block on undelivered events
  codex-manager.mjs whoami                                print the resolved Claude session id`;

const textArgument = { type: "object", properties: { text: { type: "string", description: "The message for Claude." } }, required: ["text"], additionalProperties: false };
const NOTIFY_TOOL = {
  type: "function",
  name: "notify_claude",
  description: "Send a short progress note to Claude, the supervisor who started this thread. Claude reads it asynchronously; keep working after calling it.",
  inputSchema: textArgument
};
const ASK_TOOL = {
  type: "function",
  name: "ask_claude",
  description: "Ask Claude, the supervisor who started this thread, for a decision and wait for the answer. The answer comes back as this tool's result. Use it only when you need a decision you cannot make yourself; if no answer arrives in time the result says so and you proceed on your own judgment.",
  inputSchema: textArgument
};
const DEFAULT_DECISIONS = ["accept", "acceptForSession", "decline", "cancel"];
const APPROVALS = {
  "item/commandExecution/requestApproval": ["command", "cwd", "reason"],
  "item/fileChange/requestApproval": ["reason", "grantRoot"]
};
const askTimeoutSeconds = () => Number(process.env.CODEX_MANAGER_ASK_TIMEOUT) > 0 ? Number(process.env.CODEX_MANAGER_ASK_TIMEOUT) : 300;
const RECONNECT_DELAYS_MS = [1000, 2000, 5000, 10000, 30000];

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
  return `node ${JSON.stringify(scriptPath)} await --thread ${threadId}`;
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// --- await -------------------------------------------------------------------

async function runAwait(options) {
  if (!options.thread) throw new Error("--thread is required");
  const store = new SessionStore(await resolveSessionId());
  const timeoutMs = options.timeout === undefined ? undefined : Number(options.timeout) * 1000;
  if (timeoutMs !== undefined && !(timeoutMs > 0)) throw new Error("--timeout must be a positive number of seconds");
  const started = Date.now();
  for (;;) {
    const lines = store.claim(options.thread);
    if (lines.length) {
      process.stdout.write(`${lines.join("\n")}\n`);
      return 0;
    }
    if (timeoutMs !== undefined && Date.now() - started >= timeoutMs) return 124;
    await sleep(250);
  }
}

// --- pending (Stop hook) -----------------------------------------------------

async function runPending() {
  let hook;
  try {
    hook = JSON.parse(await readStdin());
  } catch {
    return 0;
  }
  if (!hook?.session_id) return 0;
  const store = new SessionStore(hook.session_id);
  if (!store.exists()) return 0;
  const sections = [];
  for (const threadId of store.threadIds()) {
    const lines = store.claim(threadId);
    if (lines.length) sections.push(`codex thread ${threadId}:\n${lines.join("\n")}`);
  }
  // A held ask is nudged once per stop; when the hook itself caused this stop, only new events count.
  if (!hook.stop_hook_active) {
    for (const thread of store.readState().threads) {
      for (const waiting of thread.waiting ?? []) {
        sections.push(`codex thread ${thread.id} is waiting for reply since ${new Date(waiting.since).toISOString()} (${waiting.kind} ${waiting.callId}): ${waiting.text}`);
      }
    }
  }
  if (!sections.length) return 0;
  const reason = `codex-manager: ${sections.length} thread(s) need attention before you stop.\n\n${sections.join("\n\n")}`;
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
    this.cachedState = undefined;
    this.lastMessages = new Map();
    this.held = new Map();
    this.reconnectAttempt = 0;
  }

  state() {
    if (!this.cachedState) this.cachedState = this.store.readState();
    return this.cachedState;
  }

  writeState(state) {
    this.store.writeState(state);
    this.cachedState = undefined;
  }

  owns(threadId) {
    return Boolean(threadId) && this.state().threads.some((thread) => thread.id === threadId);
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
    this.writeState(state);
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
    client.onFailure = (error) => this.onDisconnect(client, error);
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
    this.reconnectAttempt = 0;
    await this.adopt(client);
    return client;
  }

  /** A dropped daemon connection is retried while there are threads to listen to. */
  onDisconnect(client, error) {
    if (this.client !== client) return;
    this.client = undefined;
    // Held requests die with the connection; the daemon replays them on resume, with the deadline kept in state.
    for (const entries of this.held.values()) for (const entry of entries) clearTimeout(entry.timer);
    this.held.clear();
    log(`daemon connection lost: ${error.message}`);
    this.scheduleReconnect();
  }

  scheduleReconnect() {
    if (!this.state().threads.length) return;
    const delay = RECONNECT_DELAYS_MS[Math.min(this.reconnectAttempt, RECONNECT_DELAYS_MS.length - 1)];
    this.reconnectAttempt += 1;
    setTimeout(() => {
      if (this.client) return;
      this.connect().catch((failure) => {
        log(`reconnect failed: ${failure.message}`);
        this.scheduleReconnect();
      });
    }, delay).unref();
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
      if (latest.id === thread.turnId && latest.status === thread.lastStatus) continue;
      if (latest.status === "inProgress") this.updateThread(thread.id, { turnId: latest.id, lastStatus: "inProgress", error: undefined, waiting: latest.id === thread.turnId ? thread.waiting : [] });
      else if (latest.id === thread.turnId && thread.lastStatus !== "inProgress") this.updateThread(thread.id, { lastStatus: latest.status, error: undefined, waiting: [] });
      else this.recordCompletion(thread.id, latest);
    }
  }

  recordCompletion(threadId, turn) {
    const fromItems = turn.items?.filter((item) => item.type === "agentMessage").at(-1)?.text;
    const lastMessage = fromItems ?? this.lastMessages.get(threadId) ?? "";
    this.lastMessages.delete(threadId);
    this.store.append(threadId, { kind: "completed", turnId: turn.id, status: turn.status, lastMessage, error: turn.error?.message });
    this.forget(threadId);
    this.updateThread(threadId, { turnId: turn.id, lastStatus: turn.status, error: undefined, waiting: [] });
  }

  /** The daemon aborts a thread's held requests when its turn ends or a new one starts. */
  forget(threadId) {
    for (const [callId, entries] of this.held) {
      if (entries[0]?.threadId !== threadId) continue;
      for (const entry of entries) clearTimeout(entry.timer);
      this.held.delete(callId);
    }
  }

  onNotification({ method, params = {} }) {
    const threadId = params.threadId;
    if (!this.owns(threadId)) return;
    if (method === "item/completed" && params.item?.type === "agentMessage") {
      this.lastMessages.set(threadId, params.item.text || "");
    } else if (method === "turn/started") {
      const thread = this.thread(threadId);
      if (params.turn?.id !== thread.turnId) this.forget(threadId);
      this.updateThread(threadId, { turnId: params.turn?.id, lastStatus: "inProgress", waiting: params.turn?.id === thread.turnId ? thread.waiting : [] });
    } else if (method === "turn/completed") {
      this.recordCompletion(threadId, params.turn);
    }
  }

  onServerRequest(client, message) {
    const { id, method, params = {} } = message;
    const reject = (text) => client.send({ id, error: { code: -32601, message: text } });
    if (!this.owns(params.threadId)) return reject(`codex-manager does not own thread ${params.threadId}`);
    if (method === "item/tool/call" && params.tool === NOTIFY_TOOL.name) {
      this.store.append(params.threadId, { kind: "notify", turnId: params.turnId, callId: params.callId, text: params.arguments?.text ?? "" });
      return client.send({ id, result: { contentItems: [{ type: "inputText", text: "Delivered to Claude." }], success: true } });
    }
    if (method === "item/tool/call" && params.tool === ASK_TOOL.name) {
      return this.hold(client, id, params.threadId, { kind: "ask", callId: params.callId, turnId: params.turnId, text: params.arguments?.text ?? "" });
    }
    const fields = APPROVALS[method];
    if (fields) {
      const details = Object.fromEntries(fields.filter((field) => params[field] != null).map((field) => [field, params[field]]));
      const text = [method.split("/")[1], details.command, details.reason].filter(Boolean).join(": ");
      // Amendment decisions carry payloads Claude cannot compose from a word; only plain ones are offered.
      const offered = params.availableDecisions?.filter((decision) => typeof decision === "string");
      const decisions = offered?.length ? offered : DEFAULT_DECISIONS;
      return this.hold(client, id, params.threadId, { kind: "approval", callId: params.approvalId ?? params.itemId, turnId: params.turnId, text, decisions, ...details });
    }
    return reject(`codex-manager does not handle ${method}`);
  }

  /** Parks a daemon request until Claude replies or the ask timeout answers for it. */
  hold(client, id, threadId, event) {
    const thread = this.thread(threadId);
    const waiting = thread.waiting ?? [];
    const known = waiting.find((entry) => entry.callId === event.callId);
    if (!known) this.store.append(threadId, event);
    const since = known?.since ?? Date.now();
    const remaining = Math.max(0, since + askTimeoutSeconds() * 1000 - Date.now());
    const timer = setTimeout(() => this.expire(threadId, event), remaining).unref();
    // A replayed request keeps its earlier copies so every daemon request id gets the one answer.
    this.held.set(event.callId, [...(this.held.get(event.callId) ?? []), { client, id, threadId, timer }]);
    if (!known) {
      const { turnId: _turnId, ...record } = event;
      this.updateThread(threadId, { waiting: [...waiting, { ...record, since }] });
    }
  }

  expire(threadId, event) {
    const seconds = askTimeoutSeconds();
    const answer = event.kind === "approval"
      ? { decision: "decline" }
      : { contentItems: [{ type: "inputText", text: `Claude did not answer within ${seconds} seconds. Proceed on your own judgment and state the assumption you made in your final message.` }], success: true };
    log(`${event.kind} ${event.callId} on thread ${threadId} timed out after ${seconds}s`);
    this.answer(threadId, event.callId, answer);
  }

  answer(threadId, callId, result) {
    for (const entry of this.held.get(callId) ?? []) {
      clearTimeout(entry.timer);
      entry.client.send({ id: entry.id, result });
    }
    this.held.delete(callId);
    const thread = this.state().threads.find((candidate) => candidate.id === threadId);
    if (thread?.waiting?.some((entry) => entry.callId === callId)) this.updateThread(threadId, { waiting: thread.waiting.filter((entry) => entry.callId !== callId) });
  }

  attention() {
    const lines = this.state().threads.flatMap((thread) => (thread.waiting ?? []).map((entry) => `thread ${thread.id} is waiting for reply (${entry.kind} ${entry.callId}): ${entry.text}`));
    return lines.length ? lines : undefined;
  }

  async start({ cwd, prompt, name }) {
    if (!cwd || !prompt) throw new Error("cwd and prompt are required");
    const client = await this.connect();
    const absolute = path.resolve(cwd);
    const started = await client.request("thread/start", {
      cwd: absolute,
      approvalPolicy: "on-request",
      sandbox: "workspace-write",
      config: { "sandbox_workspace_write.network_access": true },
      serviceName: "codex-manager",
      ephemeral: false,
      dynamicTools: [NOTIFY_TOOL, ASK_TOOL]
    });
    const threadId = started.thread.id;
    if (name) await client.request("thread/name/set", { threadId, name });
    this.updateThread(threadId, { name: name ?? null, cwd: absolute, turnId: null, lastStatus: "idle", waiting: [] });
    const turn = await this.startTurn(client, threadId, prompt);
    return { threadId, turnId: turn.id, name: name ?? null, cwd: absolute, await: awaitCommand(threadId), note: "Run the await command with run_in_background; it exits when codex finishes the turn, calls notify_claude or ask_claude, or needs an approval." };
  }

  async startTurn(client, threadId, prompt) {
    const response = await client.request("turn/start", { threadId, input: [{ type: "text", text: prompt, text_elements: [] }] });
    this.updateThread(threadId, { turnId: response.turn.id, lastStatus: "inProgress" });
    return response.turn;
  }

  async send({ threadId, prompt }) {
    if (!threadId || !prompt) throw new Error("threadId and prompt are required");
    this.thread(threadId);
    const client = await this.connect();
    const before = this.thread(threadId).turnId;
    const turn = await this.startTurn(client, threadId, prompt);
    const steered = turn.id === before;
    return { threadId, turnId: turn.id, steered, await: awaitCommand(threadId), note: steered ? "A turn was already running, so the prompt was injected into it." : "Run the await command with run_in_background." };
  }

  async reply({ threadId, callId, text }) {
    if (!threadId || !text) throw new Error("threadId and text are required");
    this.thread(threadId);
    await this.connect();
    const waiting = this.thread(threadId).waiting ?? [];
    if (!waiting.length) throw new Error(`thread ${threadId} is not waiting for a reply`);
    if (!callId && waiting.length > 1) throw new Error(`thread ${threadId} has several requests waiting; pass callId: ${waiting.map((entry) => `${entry.callId} (${entry.kind})`).join(", ")}`);
    const target = callId ? waiting.find((entry) => entry.callId === callId) : waiting[0];
    if (!target) throw new Error(`thread ${threadId} has no waiting request ${callId}`);
    const { kind, decisions } = target;
    if (!this.held.has(target.callId)) throw new Error(`request ${target.callId} was not replayed by the daemon; its turn has probably ended`);
    let result;
    if (kind === "approval") {
      if (!decisions.includes(text)) throw new Error(`approval reply must be one of: ${decisions.join(", ")}`);
      result = { decision: text };
    } else {
      result = { contentItems: [{ type: "inputText", text }], success: true };
    }
    this.answer(threadId, target.callId, result);
    return { threadId, callId: target.callId, kind, answered: true, await: awaitCommand(threadId) };
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
    const threads = this.state().threads.map((thread) => ({ waiting: [], ...thread, unread: this.store.unreadCount(thread.id), await: awaitCommand(thread.id) }));
    return { connected: this.connected(), sessionDir: this.store.dir, threads };
  }
}

const TOOLS = [
  { name: "start", description: "Start a codex worker thread on the shared daemon and give it a task. Returns the thread id and the await command to run in the background.", inputSchema: { type: "object", properties: { cwd: { type: "string", description: "Absolute working directory for the worker." }, prompt: { type: "string", description: "The task, written for a worker that sees nothing of this conversation." }, name: { type: "string", description: "Short human-readable thread name." } }, required: ["cwd", "prompt"] } },
  { name: "send", description: "Send a follow-up prompt to a codex thread. Starts a new turn when idle; when a turn is running the prompt is injected into it.", inputSchema: { type: "object", properties: { threadId: { type: "string" }, prompt: { type: "string" } }, required: ["threadId", "prompt"] } },
  { name: "reply", description: "Answer a codex thread that is waiting: the text becomes the result of its ask_claude call, or, for an approval request, one of the decisions its inbox event listed. callId is needed only when several requests are waiting.", inputSchema: { type: "object", properties: { threadId: { type: "string" }, callId: { type: "string" }, text: { type: "string" } }, required: ["threadId", "text"] } },
  { name: "interrupt", description: "Interrupt the running turn of a codex thread.", inputSchema: { type: "object", properties: { threadId: { type: "string" } }, required: ["threadId"] } },
  { name: "list", description: "List the codex threads this session started, their last turn status, what each is waiting for, and how many inbox events are still unread.", inputSchema: { type: "object", properties: {} } }
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
        const attention = manager.attention();
        return reply(id, { result: { content: [{ type: "text", text: JSON.stringify(attention ? { ...result, attention } : result, null, 2) }] } });
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
