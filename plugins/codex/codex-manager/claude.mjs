// ABOUTME: The MCP server codex runs to reach Claude: notify_claude and ask_claude, over stdio.
// ABOUTME: Each call names its codex thread; the thread's supervisor record says which Claude session gets it.

import crypto from "node:crypto";
import readline from "node:readline";
import { SessionStore, askTimeoutSeconds, readSupervisor, running } from "./inbox.mjs";

const textArgument = { type: "object", properties: { text: { type: "string", description: "The message for Claude." } }, required: ["text"], additionalProperties: false };
const TOOLS = [
  { name: "notify_claude", description: "Send a short progress note to Claude, the supervisor of this thread. Claude reads it asynchronously; keep working after calling it. Fails at once when no Claude session supervises this thread.", inputSchema: textArgument },
  { name: "ask_claude", description: "Ask Claude, the supervisor of this thread, for a decision and wait for the answer. The answer comes back as this tool's result. Use it only when you need a decision you cannot make yourself; if no answer arrives in time the result says so and you proceed on your own judgment. Fails at once when no Claude session supervises this thread.", inputSchema: textArgument }
];
const POLL_MS = 250;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const said = (text) => ({ content: [{ type: "text", text }] });
const failed = (text) => ({ ...said(text), isError: true });

/** The store of the Claude session whose manager is running and has claimed the thread. */
function supervisorStore(threadId) {
  const supervisor = threadId ? readSupervisor(threadId) : undefined;
  return supervisor && running(supervisor.pid) ? new SessionStore(supervisor.sessionId) : undefined;
}

/** Questions this process is waiting on, withdrawn when codex goes away. */
const open = new Set();

async function ask(store, threadId, event, cancelled) {
  const withdraw = () => store.removeAsk(threadId, event.callId);
  open.add(withdraw);
  try {
    return await answerTo(store, threadId, event, cancelled);
  } finally {
    open.delete(withdraw);
  }
}

async function answerTo(store, threadId, event, cancelled) {
  const seconds = askTimeoutSeconds();
  const since = Date.now();
  store.writeAsk(threadId, { kind: "ask", callId: event.callId, since, text: event.text });
  store.append(threadId, { kind: "ask", ...event });
  for (;;) {
    const reply = store.takeReply(threadId, event.callId);
    if (reply) return said(reply.text);
    if (!store.hasAsk(threadId, event.callId)) return failed("The turn ended before Claude answered.");
    if (cancelled()) {
      store.removeAsk(threadId, event.callId);
      return failed("The call was cancelled.");
    }
    if (Date.now() - since >= seconds * 1000) {
      store.removeAsk(threadId, event.callId);
      return said(`Claude did not answer within ${seconds} seconds. Proceed on your own judgment and state the assumption you made in your final message.`);
    }
    await sleep(POLL_MS);
  }
}

async function callTool({ name, arguments: args = {}, _meta: meta = {} }, cancelled) {
  if (!TOOLS.some((tool) => tool.name === name)) return failed(`unknown tool: ${name}`);
  const store = supervisorStore(meta.threadId);
  if (!store) return failed("No Claude session is supervising this thread. Proceed on your own judgment.");
  const event = { turnId: meta["x-codex-turn-metadata"]?.turn_id, callId: meta.callId ?? crypto.randomUUID(), text: args.text ?? "" };
  if (name === "ask_claude") return ask(store, meta.threadId, event, cancelled);
  store.append(meta.threadId, { kind: "notify", ...event });
  return said("Delivered to Claude.");
}

export async function runClaude(version) {
  const reply = (id, body) => process.stdout.write(`${JSON.stringify({ jsonrpc: "2.0", id, ...body })}\n`);
  const cancelled = new Set();
  const handle = async ({ id, method, params = {} }) => {
    if (method === "notifications/cancelled") return cancelled.add(params.requestId);
    if (id === undefined) return undefined;
    if (method === "initialize") return reply(id, { result: { protocolVersion: params.protocolVersion || "2025-06-18", capabilities: { tools: {} }, serverInfo: { name: "claude", version } } });
    if (method === "ping") return reply(id, { result: {} });
    if (method === "tools/list") return reply(id, { result: { tools: TOOLS } });
    if (method === "tools/call") return reply(id, { result: await callTool(params, () => cancelled.delete(id)) });
    return reply(id, { error: { code: -32601, message: `method not found: ${method}` } });
  };

  const inflight = new Set();
  for await (const line of readline.createInterface({ input: process.stdin, crlfDelay: Infinity })) {
    if (!line.trim()) continue;
    let message;
    try {
      message = JSON.parse(line);
    } catch {
      process.stderr.write(`claude: ignoring invalid JSON: ${line.slice(0, 80)}\n`);
      continue;
    }
    const work = handle(message).catch((error) => process.stderr.write(`claude: ${error.stack || error.message}\n`));
    inflight.add(work);
    work.finally(() => inflight.delete(work));
  }
  // Codex closed the pipe, so nobody is left to read the answers still being waited for.
  for (const withdraw of open) withdraw();
  return 0;
}
