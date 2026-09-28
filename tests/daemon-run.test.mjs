// ABOUTME: Tests the codex app-server daemon task runner over a fake unix WebSocket server.
// ABOUTME: Covers RPC sequencing, outcomes, timeout interruption, and WebSocket framing edge cases.

import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import test from "node:test";
import { fakeDaemon, send, serverFrame } from "./helpers/fake-daemon.mjs";

const runner = path.resolve("plugins/codex/skills/implement/daemon-run.mjs");
function baseScript({ onTurn, threadId = "thread-123", turnId = "turn-456" } = {}) {
  const messages = [];
  return {
    messages,
    handler(message, socket) {
      messages.push(message);
      if (message.id && message.method === "initialize") send(socket, { id: message.id, result: {} });
      if (message.id && message.method === "thread/start") send(socket, { id: message.id, result: { thread: { id: threadId } } });
      if (message.id && message.method === "thread/name/set") send(socket, { id: message.id, result: {} });
      if (message.id && message.method === "turn/start") {
        send(socket, { id: message.id, result: { turn: { id: turnId } } });
        setImmediate(() => onTurn?.(socket, { threadId, turnId }));
      }
    }
  };
}

async function runRunner(socketPath, { timeout = "5", output, cwd = ".", name = "feature/w1 a1", prompt = "Do the task", sandbox } = {}) {
  const args = [runner, "-C", cwd, "-o", output, "--name", name, "--timeout", timeout];
  if (sandbox) args.push("-s", sandbox);
  args.push(prompt);
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, args, {
      cwd: process.cwd(),
      env: { ...process.env, IMPLEMENT_DAEMON_SOCKET: socketPath },
      stdio: ["ignore", "pipe", "pipe"]
    });
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.on("error", reject);
    child.on("close", (code) => resolve({ code, stdout, stderr }));
  });
}

test("happy path uses the exact RPC sequence and writes the final agent message", async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "daemon-output-test-"));
  const output = path.join(directory, "last-message.txt");
  const script = baseScript({ onTurn(socket, ids) {
    send(socket, { method: "item/completed", params: { ...ids, item: { type: "agentMessage", id: "item-1", text: "Finished cleanly" } } });
    send(socket, { method: "turn/completed", params: { threadId: ids.threadId, turn: { id: ids.turnId, status: "completed" } } });
  } });
  const daemon = await fakeDaemon(script.handler);
  try {
    const result = await runRunner(daemon.socketPath, { output, cwd: "plugins", sandbox: "read-only" });
    assert.equal(result.code, 0, result.stderr);
    assert.deepEqual(script.messages.map((message) => message.method), ["initialize", "initialized", "thread/start", "thread/name/set", "turn/start"]);
    const start = script.messages[2].params;
    assert.deepEqual(start, { cwd: path.resolve("plugins"), approvalPolicy: "never", sandbox: "read-only", serviceName: "codex-implement", ephemeral: false });
    assert.deepEqual(script.messages[3].params, { threadId: "thread-123", name: "feature/w1 a1" });
    assert.equal(await readFile(output, "utf8"), "Finished cleanly");
    assert.match(result.stdout, /^\[thread thread-123\] feature\/w1 a1/m);
    assert.match(result.stdout, /^\[turn\] completed$/m);
  } finally {
    await daemon.close();
    await rm(directory, { recursive: true, force: true });
  }
});

test("failed turn exits 1 and reports its error", async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "daemon-output-test-"));
  const script = baseScript({ onTurn(socket, ids) {
    send(socket, { method: "turn/completed", params: { threadId: ids.threadId, turn: { id: ids.turnId, status: "failed", error: { message: "task exploded" } } } });
  } });
  const daemon = await fakeDaemon(script.handler);
  try {
    const result = await runRunner(daemon.socketPath, { output: path.join(directory, "last.txt") });
    assert.equal(result.code, 1, result.stderr);
    assert.match(result.stderr, /task exploded/);
  } finally {
    await daemon.close();
    await rm(directory, { recursive: true, force: true });
  }
});

test("timeout interrupts the active turn and exits 124", async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "daemon-output-test-"));
  let interrupt;
  const script = baseScript();
  const handler = (message, socket) => {
    script.handler(message, socket);
    if (message.method === "turn/interrupt") {
      interrupt = message.params;
      send(socket, { id: message.id, result: {} });
      send(socket, { method: "turn/completed", params: { threadId: "thread-123", turn: { id: "turn-456", status: "interrupted" } } });
    }
  };
  const daemon = await fakeDaemon(handler);
  try {
    const result = await runRunner(daemon.socketPath, { output: path.join(directory, "last.txt"), timeout: "1" });
    assert.equal(result.code, 124, result.stderr);
    assert.deepEqual(interrupt, { threadId: "thread-123", turnId: "turn-456" });
  } finally {
    await daemon.close();
    await rm(directory, { recursive: true, force: true });
  }
});

test("answers server ping with an identical pong payload", async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "daemon-output-test-"));
  const ping = Buffer.from("still-there?");
  let ids;
  const script = baseScript({ onTurn(socket, turnIds) {
    ids = turnIds;
    socket.write(serverFrame(ping, { opcode: 9 }));
  } });
  const daemon = await fakeDaemon(script.handler, (received, socket) => {
    if (received.opcode !== 10) return;
    assert.deepEqual(received.payload, ping);
    send(socket, { method: "turn/completed", params: { threadId: ids.threadId, turn: { id: ids.turnId, status: "completed" } } });
  });
  try {
    const result = await runRunner(daemon.socketPath, { output: path.join(directory, "last.txt") });
    assert.equal(result.code, 0, result.stderr);
  } finally {
    await daemon.close();
    await rm(directory, { recursive: true, force: true });
  }
});

test("decodes a split 64-bit-length server message", async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "daemon-output-test-"));
  const finalText = "x".repeat(70_000);
  const script = baseScript({ onTurn(socket, ids) {
    const message = serverFrame(JSON.stringify({ method: "item/completed", params: { ...ids, item: { type: "agentMessage", id: "large", text: finalText } } }));
    socket.write(message.subarray(0, 137));
    setImmediate(() => {
      socket.write(message.subarray(137));
      send(socket, { method: "turn/completed", params: { threadId: ids.threadId, turn: { id: ids.turnId, status: "completed" } } });
    });
  } });
  const daemon = await fakeDaemon(script.handler);
  try {
    const output = path.join(directory, "last.txt");
    const result = await runRunner(daemon.socketPath, { output });
    assert.equal(result.code, 0, result.stderr);
    assert.equal(await readFile(output, "utf8"), finalText);
  } finally {
    await daemon.close();
    await rm(directory, { recursive: true, force: true });
  }
});

test("reassembles continuation frames", async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "daemon-output-test-"));
  const finalText = "fragmented message";
  const script = baseScript({ onTurn(socket, ids) {
    const payload = Buffer.from(JSON.stringify({ method: "item/completed", params: { ...ids, item: { type: "agentMessage", id: "fragmented", text: finalText } } }));
    const middle = Math.floor(payload.length / 2);
    socket.write(serverFrame(payload.subarray(0, middle), { finished: false }));
    socket.write(serverFrame(payload.subarray(middle), { opcode: 0 }));
    send(socket, { method: "turn/completed", params: { threadId: ids.threadId, turn: { id: ids.turnId, status: "completed" } } });
  } });
  const daemon = await fakeDaemon(script.handler);
  try {
    const output = path.join(directory, "last.txt");
    const result = await runRunner(daemon.socketPath, { output });
    assert.equal(result.code, 0, result.stderr);
    assert.equal(await readFile(output, "utf8"), finalText);
  } finally {
    await daemon.close();
    await rm(directory, { recursive: true, force: true });
  }
});

test("timeout during setup exits 124 without a turn to interrupt", { timeout: 10_000 }, async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "daemon-output-test-"));
  const messages = [];
  const daemon = await fakeDaemon((message) => { messages.push(message); });
  try {
    const result = await runRunner(daemon.socketPath, { output: path.join(directory, "last.txt"), timeout: "1" });
    assert.equal(result.code, 124, result.stderr);
    assert.deepEqual(messages.map((message) => message.method), ["initialize"]);
  } finally {
    await daemon.close();
    await rm(directory, { recursive: true, force: true });
  }
});

test("rejected setup request exits 1 promptly", { timeout: 10_000 }, async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "daemon-output-test-"));
  const daemon = await fakeDaemon((message, socket) => {
    if (message.method === "initialize") send(socket, { id: message.id, result: {} });
    if (message.method === "thread/start") send(socket, { id: message.id, error: { code: -32000, message: "cwd is not a directory" } });
  });
  try {
    const result = await runRunner(daemon.socketPath, { output: path.join(directory, "last.txt") });
    assert.equal(result.code, 1);
    assert.match(result.stderr, /cwd is not a directory/);
  } finally {
    await daemon.close();
    await rm(directory, { recursive: true, force: true });
  }
});

test("missing socket exits 3 with a clear connection error", async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "daemon-output-test-"));
  try {
    const result = await runRunner(path.join(directory, "missing.sock"), { output: path.join(directory, "last.txt") });
    assert.equal(result.code, 3);
    assert.match(result.stderr, /cannot connect to daemon/);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
