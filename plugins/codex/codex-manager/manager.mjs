#!/usr/bin/env node
// ABOUTME: Lets Claude Code run codex threads on the shared app-server daemon as supervised workers.
// ABOUTME: `mcp` serves Claude's tools and relays codex events; `await` and `pending` deliver them; `claude` serves codex.

import { mkdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import path from "node:path";
import readline from "node:readline";
import { fileURLToPath } from "node:url";
import { WebSocketClient, daemonSocketPath } from "../lib/daemon-client.mjs";
import { readStdin } from "../lib/stdin.mjs";
import { runClaude } from "./claude.mjs";
import { SessionStore, askTimeoutSeconds, writeSupervisor } from "./inbox.mjs";
import { parentProcesses, recordedSessionId, resolveSessionId } from "./session.mjs";

const scriptPath = fileURLToPath(import.meta.url);
const pluginManifestPath = path.resolve(path.dirname(scriptPath), "../.claude-plugin/plugin.json");
const adversarialStancePath = path.join(path.dirname(scriptPath), "adversarial-review.md");
const usage = `usage:
  manager.mjs mcp                                   serve Claude's codex tools over stdio
  manager.mjs await --thread <id> [--timeout <s>]   print the next inbox events and exit
  manager.mjs pending                               Stop hook: block on undelivered events and unwatched threads
  manager.mjs claude                                serve notify_claude and ask_claude to codex over stdio
  manager.mjs whoami                                print the resolved Claude session id`;

const DEFAULT_DECISIONS = ["accept", "acceptForSession", "decline", "cancel"];
const APPROVALS = {
  "item/commandExecution/requestApproval": ["command", "cwd", "reason"],
  "item/fileChange/requestApproval": ["reason", "grantRoot"]
};
// Codex gives up on a tool call after its own timeout, so that one has to outlast the wait for Claude's answer.
const TOOL_TIMEOUT_MARGIN_SECONDS = 60;
const FORWARDED_ENV = ["CODEX_MANAGER_STATE_DIR", "CODEX_MANAGER_REPLY_TIMEOUT_SECONDS"];

/** How codex launches the server that carries notify_claude and ask_claude, as an mcp_servers entry. */
function claudeToolsServer() {
  const env = Object.fromEntries(FORWARDED_ENV.filter((name) => process.env[name]).map((name) => [name, process.env[name]]));
  return { command: process.execPath, args: [scriptPath, "claude"], env, tool_timeout_sec: askTimeoutSeconds() + TOOL_TIMEOUT_MARGIN_SECONDS, default_tools_approval_mode: "approve" };
}
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
  store.markAwait(options.thread);
  try {
    for (;;) {
      const lines = store.claim(options.thread);
      if (lines.length) {
        process.stdout.write(`${lines.join("\n")}\n`);
        return 0;
      }
      if (timeoutMs !== undefined && Date.now() - started >= timeoutMs) return 124;
      await sleep(250);
    }
  } finally {
    store.clearAwait(options.thread);
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
      for (const waiting of [...(thread.waiting ?? []), ...store.asks(thread.id)]) {
        sections.push(`codex thread ${thread.id} is waiting for reply since ${new Date(waiting.since).toISOString()} (${waiting.kind} ${waiting.callId}): ${waiting.text}`);
      }
    }
  }
  // Only an await process wakes Claude once it has stopped, and starting one lifts this block, so it holds on every stop.
  for (const thread of store.readState().threads) {
    if (thread.lastStatus !== "inProgress" || store.awaiting(thread.id).length) continue;
    sections.push(`codex thread ${thread.id} is running and nothing is waiting for it. Run this with run_in_background before you stop:\n${awaitCommand(thread.id)}`);
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
    if (index === -1) {
      state.threads.push({ id: threadId, ...patch });
      this.supervise(threadId);
    } else state.threads[index] = { ...state.threads[index], ...patch };
    this.writeState(state);
  }

  /** Points the thread's notify_claude and ask_claude calls at this session. */
  supervise(threadId) {
    writeSupervisor(threadId, { sessionId: this.store.sessionId, pid: process.pid });
  }

  /** What the thread is waiting on: approvals held for the daemon, and questions codex keeps on disk. */
  waiting(thread) {
    return [...(thread.waiting ?? []), ...this.store.asks(thread.id)];
  }

  dropThread(threadId) {
    const state = this.state();
    state.threads = state.threads.filter((thread) => thread.id !== threadId);
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
    // Held approvals die with the connection; the daemon replays them on resume, with the deadline kept in state.
    for (const entries of this.held.values()) for (const entry of entries) clearTimeout(entry.timer);
    this.held.clear();
    log(`daemon connection lost: ${error.message}`);
    this.scheduleReconnect();
  }

  scheduleReconnect() {
    if (!this.listening()) return;
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

  /**
   * Subscribes to a thread and returns its latest turn. The turns a resume returns show a finished
   * review as still running, so the turn is listed instead.
   */
  async subscribe(client, threadId) {
    await client.request("thread/resume", { threadId });
    const turns = await client.request("thread/turns/list", { threadId, limit: 1, sortDirection: "desc", itemsView: "full" });
    return turns.data?.[0];
  }

  /** Subscribes to a detached thread again, which also loads it when the daemon has unloaded it. */
  async listenAgain(client, threadId) {
    if (!this.thread(threadId).detached) return;
    await this.subscribe(client, threadId);
    this.updateThread(threadId, { detached: undefined });
  }

  /** Whether any recorded thread is still listened to; detached ones need no daemon connection. */
  listening() {
    return this.state().threads.some((thread) => !thread.detached);
  }

  /** Re-subscribes to every recorded thread Claude still listens to and backfills turns that ended while disconnected. */
  async adopt(client) {
    for (const thread of this.state().threads) {
      if (thread.detached) continue;
      this.supervise(thread.id);
      let latest;
      try {
        latest = await this.subscribe(client, thread.id);
      } catch (error) {
        log(`cannot resume thread ${thread.id}: ${error.message}`);
        this.updateThread(thread.id, { lastStatus: "unavailable", error: error.message });
        continue;
      }
      if (!latest) continue;
      if (latest.id === thread.turnId && latest.status === thread.lastStatus) continue;
      if (latest.status === "inProgress") this.updateThread(thread.id, { turnId: latest.id, lastStatus: "inProgress", error: undefined, waiting: latest.id === thread.turnId ? thread.waiting : [] });
      else if (latest.id === thread.turnId && thread.lastStatus !== "inProgress") this.updateThread(thread.id, { lastStatus: latest.status, error: undefined, waiting: [] });
      else this.recordCompletion(thread.id, latest);
    }
  }

  /** A review thread's completion points at the file, not at the review text itself. */
  saveReview(threadId, item) {
    const out = this.thread(threadId).review;
    if (!out) return;
    mkdirSync(path.dirname(out), { recursive: true });
    writeFileSync(out, item.review ?? "");
    this.lastMessages.set(threadId, `Review written to ${out}`);
  }

  recordCompletion(threadId, turn) {
    const review = turn.items?.find((item) => item.type === "exitedReviewMode");
    if (review) this.saveReview(threadId, review);
    // A review thread's agent message repeats the review, which the file already holds.
    const fromItems = this.thread(threadId).review ? undefined : turn.items?.filter((item) => item.type === "agentMessage").at(-1)?.text;
    const lastMessage = fromItems ?? this.lastMessages.get(threadId) ?? "";
    this.lastMessages.delete(threadId);
    this.store.append(threadId, { kind: "completed", turnId: turn.id, status: turn.status, lastMessage, error: turn.error?.message });
    this.forget(threadId);
    this.updateThread(threadId, { turnId: turn.id, lastStatus: turn.status, error: undefined, waiting: [] });
  }

  /** The daemon aborts a thread's held requests when its turn ends or a new one starts, and its questions go with them. */
  forget(threadId) {
    for (const ask of this.store.asks(threadId)) this.store.removeAsk(threadId, ask.callId);
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
      if (!this.thread(threadId).review) this.lastMessages.set(threadId, params.item.text || "");
    } else if (method === "item/completed" && params.item?.type === "exitedReviewMode") {
      this.saveReview(threadId, params.item);
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
    // Every subscriber receives an attached session's requests and the first answer settles them, errors included.
    if (this.thread(params.threadId).attached) return undefined;
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

  /** Parks an approval request until Claude replies or the ask timeout declines it. */
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
    log(`${event.kind} ${event.callId} on thread ${threadId} timed out after ${seconds}s`);
    this.answer(threadId, event.callId, { decision: "decline" });
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
    const lines = this.state().threads.flatMap((thread) => this.waiting(thread).map((entry) => `thread ${thread.id} is waiting for reply (${entry.kind} ${entry.callId}): ${entry.text}`));
    return lines.length ? lines : undefined;
  }

  async startThread(client, { cwd, name, sandbox, claudeTools, record }) {
    const params = { cwd, approvalPolicy: "on-request", sandbox, serviceName: "codex-manager", ephemeral: false };
    if (sandbox === "workspace-write") params.config = { "sandbox_workspace_write.network_access": true };
    if (claudeTools) params.config = { ...params.config, "mcp_servers.claude": claudeToolsServer() };
    const started = await client.request("thread/start", params);
    const threadId = started.thread.id;
    if (name) await client.request("thread/name/set", { threadId, name });
    this.updateThread(threadId, { name: name ?? null, cwd, turnId: null, lastStatus: "idle", waiting: [], ...record });
    return threadId;
  }

  async start({ cwd, prompt, name }) {
    if (!cwd || !prompt) throw new Error("cwd and prompt are required");
    const client = await this.connect();
    const absolute = path.resolve(cwd);
    const threadId = await this.startThread(client, { cwd: absolute, name, sandbox: "workspace-write", claudeTools: true });
    const turn = await this.startTurn(client, threadId, prompt);
    return { threadId, turnId: turn.id, name: name ?? null, cwd: absolute, await: awaitCommand(threadId), note: "Run the await command with run_in_background; it exits when codex finishes the turn, calls notify_claude or ask_claude, or needs an approval." };
  }

  /** Runs codex's own review mode (its rubric and priorities) over the commits since base; the rendered review lands in out. */
  async review({ cwd, base, plan, decisions, out, name, stance, focus }) {
    if (!cwd || !base || !out) throw new Error("cwd, base and out are required");
    if (stance !== undefined && stance !== "adversarial") throw new Error('stance must be "adversarial" when given');
    const client = await this.connect();
    const absolute = path.resolve(cwd);
    const target = path.resolve(out);
    const threadId = await this.startThread(client, { cwd: absolute, name, sandbox: "read-only", record: { review: target } });
    const instructions = reviewInstructions(base, plan, decisions, stance, focus);
    const response = await client.request("review/start", { threadId, target: { type: "custom", instructions }, delivery: "inline" });
    this.updateThread(threadId, { turnId: response.turn.id, lastStatus: "inProgress" });
    return { threadId, turnId: response.turn.id, name: name ?? null, cwd: absolute, out: target, await: awaitCommand(threadId), note: "Run the await command with run_in_background; it exits when the review is written." };
  }

  /** Finds a session on the daemon by id or exact name, the open one first; the daemon's own searchTerm filters titles, not names. */
  async findThread(client, wanted) {
    const named = [];
    let cursor;
    do {
      const page = await client.request("thread/list", { limit: 100, sortKey: "updated_at", cursor });
      for (const thread of page.data ?? []) {
        if (thread.id === wanted) return thread;
        if (thread.name === wanted) named.push(thread);
      }
      cursor = page.nextCursor;
    } while (cursor);
    if (!named.length) throw new Error(`no codex session has the id or name ${JSON.stringify(wanted)}`);
    // Sessions keep their name after they are closed, so the one that is open is the one meant.
    const open = named.filter((thread) => thread.status?.type !== "notLoaded");
    if (open.length === 1) return open[0];
    if (named.length > 1) throw new Error(`${named.length} codex sessions are named ${JSON.stringify(wanted)}; pass the id of one: ${named.map((thread) => `${thread.id} (${thread.status?.type}, ${thread.cwd})`).join(", ")}`);
    return named[0];
  }

  /** Subscribes to a session this manager did not start, leaving its settings as its own client set them. */
  async attach({ thread }) {
    if (!thread) throw new Error("thread is required");
    const client = await this.connect();
    const found = await this.findThread(client, thread);
    if (!this.owns(found.id)) {
      // Recorded before the resume so a turn that ends right behind the response is not missed.
      this.updateThread(found.id, { name: found.name ?? null, cwd: found.cwd, turnId: null, lastStatus: "idle", waiting: [], attached: true });
      let latest;
      try {
        latest = await this.subscribe(client, found.id);
      } catch (error) {
        this.dropThread(found.id);
        throw error;
      }
      if (latest && this.thread(found.id).turnId === null) this.updateThread(found.id, { turnId: latest.id, lastStatus: latest.status });
    } else await this.listenAgain(client, found.id);
    const { name, cwd, lastStatus, attached } = this.thread(found.id);
    return { threadId: found.id, name, cwd, attached: Boolean(attached), lastStatus, await: awaitCommand(found.id), note: "Give it a prompt with send, then run the await command with run_in_background; it exits when codex finishes a turn. Approvals and questions stay with the client the session runs in." };
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
    await this.listenAgain(client, threadId);
    const before = this.thread(threadId).turnId;
    const turn = await this.startTurn(client, threadId, prompt);
    const steered = turn.id === before;
    return { threadId, turnId: turn.id, steered, await: awaitCommand(threadId), note: steered ? "A turn was already running, so the prompt was injected into it." : "Run the await command with run_in_background." };
  }

  async reply({ threadId, callId, text }) {
    if (!threadId || !text) throw new Error("threadId and text are required");
    this.thread(threadId);
    await this.connect();
    const waiting = this.waiting(this.thread(threadId));
    if (!waiting.length) throw new Error(`thread ${threadId} is not waiting for a reply`);
    if (!callId && waiting.length > 1) throw new Error(`thread ${threadId} has several requests waiting; pass callId: ${waiting.map((entry) => `${entry.callId} (${entry.kind})`).join(", ")}`);
    const target = callId ? waiting.find((entry) => entry.callId === callId) : waiting[0];
    if (!target) throw new Error(`thread ${threadId} has no waiting request ${callId}`);
    const { kind, decisions } = target;
    if (kind === "ask") {
      this.store.writeReply(threadId, target.callId, text);
    } else {
      if (!this.held.has(target.callId)) throw new Error(`request ${target.callId} was not replayed by the daemon; its turn has probably ended`);
      if (!decisions.includes(text)) throw new Error(`approval reply must be one of: ${decisions.join(", ")}`);
      this.answer(threadId, target.callId, { decision: text });
    }
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

  /** Stops listening to a thread whose work is done, so the daemon unloads it once nobody else is subscribed. */
  async detach({ threadId }) {
    if (!threadId) throw new Error("threadId is required");
    if (this.thread(threadId).lastStatus === "inProgress") throw new Error(`thread ${threadId} is still running a turn; wait for it or interrupt it first`);
    const client = await this.connect();
    const { status } = await client.request("thread/unsubscribe", { threadId });
    this.updateThread(threadId, { detached: true });
    return { threadId, detached: true, status };
  }

  async list() {
    if (!this.client) await this.connect().catch((error) => log(`not connected: ${error.message}`));
    const threads = this.state().threads.map((thread) => ({ ...thread, waiting: this.waiting(thread), unread: this.store.unreadCount(thread.id), await: awaitCommand(thread.id) }));
    return { connected: this.connected(), sessionDir: this.store.dir, threads };
  }
}

/** Codex's own base-branch review wording, pointed at a commit and, when given, at the spec the changes implement. */
function reviewInstructions(base, plan, decisions, stance, focus) {
  const spec = plan ? ` The changes implement the spec at ${plan}; read it first, and do not flag a behaviour change the spec asks for.` : "";
  const decided = decisions ? ` The rules decided while the changes were built are in ${decisions}; read it too. They are not part of the spec and nobody has reviewed them: do not flag one as a departure from the spec, and judge each rule in it on its own. For each, trace what the rule makes the code return or store and what reads that, also in code the changes did not touch; when it leads to a wrong result for a caller or a user, report it as a finding that names the rule.` : "";
  const adversarial = stance === "adversarial" ? `\n\n${readFileSync(adversarialStancePath, "utf8").trim()}` : "";
  const focused = focus ? `\n\nFocus: ${focus}. Weigh it heavily, and still report any other material issue.` : "";
  const close = adversarial || focused ? "\n\n" : " ";
  return `Review the code changes since commit ${base}. Run \`git diff ${base}\` to inspect the changes.${spec}${decided}${adversarial}${focused}${close}Provide prioritized, actionable findings.`;
}

const INSTRUCTIONS = `Use start to create a thread, then run the returned await command in the background (run_in_background).
Answer asks with reply; await can return an ask already answered, so check list for waiting requests first.
A thread is finished only when its turn completed (not failed, interrupted or waiting on an ask), you have checked and accepted its result, and you plan no further message. Blocked threads and results still being checked are not finished.
Once finished, detach it in the same turn; send re-attaches it, so detaching an accepted thread early is safe.
Before ending a multi-thread run, list the threads and detach every finished thread.`;

const TOOLS = [
  { name: "start", description: "Start a codex worker thread on the shared daemon and give it a task. Returns the thread id and the await command to run in the background.", inputSchema: { type: "object", properties: { cwd: { type: "string", description: "Absolute working directory for the worker." }, prompt: { type: "string", description: "The task, written for a worker that sees nothing of this conversation." }, name: { type: "string", description: "Short human-readable thread name." } }, required: ["cwd", "prompt"] } },
  { name: "attach", description: "Take over a codex session that is already running elsewhere, found by its thread id or exact name, so send, interrupt and the await command work on it. Its approvals stay with the client it runs in.", inputSchema: { type: "object", properties: { thread: { type: "string", description: "Thread id or exact session name." } }, required: ["thread"] } },
  { name: "send", description: "Send a follow-up prompt to a codex thread. Starts a new turn when idle; when a turn is running the prompt is injected into it.", inputSchema: { type: "object", properties: { threadId: { type: "string" }, prompt: { type: "string" } }, required: ["threadId", "prompt"] } },
  { name: "reply", description: "Answer a codex thread that is waiting: the text becomes the result of its ask_claude call, or, for an approval request, one of the decisions its inbox event listed. callId is needed only when several requests are waiting.", inputSchema: { type: "object", properties: { threadId: { type: "string" }, callId: { type: "string" }, text: { type: "string" } }, required: ["threadId", "text"] } },
  { name: "interrupt", description: "Interrupt the running turn of a codex thread.", inputSchema: { type: "object", properties: { threadId: { type: "string" } }, required: ["threadId"] } },
  { name: "list", description: "List the codex threads this session started or attached, their last turn status, what each is waiting for, and how many inbox events are still unread.", inputSchema: { type: "object", properties: {} } },
  { name: "detach", description: "Stop listening to a codex thread once its work is completely finished and you will not prompt it again. The daemon then unloads it, so it leaves the Ready list of `codex agents`. The session is kept, and a later send picks it up again. Refused while a turn is running.", inputSchema: { type: "object", properties: { threadId: { type: "string" } }, required: ["threadId"] } },
  { name: "review", description: "Run codex's built-in review mode over the commits since a base sha in a read-only thread and save the rendered review (priority-tagged findings and an overall verdict) to a file; stance adversarial makes it a challenge review. Returns the thread id and the await command.", inputSchema: { type: "object", properties: { cwd: { type: "string", description: "Absolute path of the checkout to review." }, base: { type: "string", description: "Commit the changes start after (excluded)." }, plan: { type: "string", description: "Path of the plan or spec the changes implement, relative to cwd." }, decisions: { type: "string", description: "Path of the file listing the rules decided while the changes were built, relative to cwd or absolute." }, out: { type: "string", description: "File that receives the review text." }, name: { type: "string", description: "Short human-readable thread name." }, stance: { type: "string", enum: ["adversarial"], description: "adversarial: the reviewer looks for the strongest reasons the change should not ship and questions the approach itself. Omit for the plain review." }, focus: { type: "string", description: "What the reviewer should weigh most, in a phrase." } }, required: ["cwd", "base", "out"] } }
];

async function runMcp() {
  const plugin = JSON.parse(await readFile(pluginManifestPath, "utf8"));
  const sessionId = await resolveSessionId();
  const store = new SessionStore(sessionId);
  const manager = new Manager(store, daemonSocketPath(process.env.CODEX_MANAGER_SOCKET_FILE), plugin.version);
  log(`session ${sessionId}, state in ${store.dir}`);
  const parents = parentProcesses();
  if (manager.listening()) manager.connect().catch((error) => log(`adoption deferred: ${error.message}`));

  const reply = (id, body) => process.stdout.write(`${JSON.stringify({ jsonrpc: "2.0", id, ...body })}\n`);
  const handle = async (message) => {
    const { id, method, params = {} } = message;
    if (method === "initialize") return reply(id, { result: { protocolVersion: params.protocolVersion || "2025-06-18", capabilities: { tools: {} }, serverInfo: { name: "codex-manager", version: plugin.version }, instructions: INSTRUCTIONS } });
    if (method === "ping") return reply(id, { result: {} });
    if (method === "tools/list") return reply(id, { result: { tools: TOOLS } });
    if (method === "tools/call") {
      const tool = TOOLS.find((candidate) => candidate.name === params.name);
      if (!tool) return reply(id, { error: { code: -32602, message: `unknown tool: ${params.name}` } });
      try {
        // This process keeps the session id it started with; the await command and the Stop hook use the current one.
        const current = await recordedSessionId({ parents });
        if (current && current !== sessionId) store.alias(current);
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
  if (command === "claude") return runClaude(JSON.parse(await readFile(pluginManifestPath, "utf8")).version);
  if (command === "whoami") {
    process.stdout.write(`${await resolveSessionId()}\n`);
    return 0;
  }
  throw new Error(command ? `unknown command: ${command}` : "missing command");
}

// ~/.claude may be a symlink (dotfiles), so the invoked path is compared by its real location.
function invokedDirectly() {
  try {
    return Boolean(process.argv[1]) && realpathSync(process.argv[1]) === scriptPath;
  } catch {
    return false;
  }
}

if (invokedDirectly()) {
  try {
    process.exitCode = await main(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`${error.message}\n${usage}\n`);
    process.exitCode = error.exitCode || 2;
  }
}
