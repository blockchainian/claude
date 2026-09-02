// ABOUTME: Tests the codex app-server daemon task runner over a fake unix WebSocket server.
// ABOUTME: Covers RPC sequencing, outcomes, timeout interruption, and WebSocket framing edge cases.

import assert from "node:assert/strict";
import crypto from "node:crypto";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import test from "node:test";

const runner = path.resolve("plugins/codex/skills/execute/daemon-run.mjs");
const websocketGuid = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

function serverFrame(payload, { opcode = 1, finished = true } = {}) {
  payload = Buffer.isBuffer(payload) ? payload : Buffer.from(payload);
  const extended = payload.length < 126 ? 0 : payload.length < 65536 ? 2 : 8;
  const header = Buffer.alloc(2 + extended);
  header[0] = (finished ? 0x80 : 0) | opcode;
  header[1] = extended === 0 ? payload.length : extended === 2 ? 126 : 127;
  if (extended === 2) header.writeUInt16BE(payload.length, 2);
  if (extended === 8) header.writeBigUInt64BE(BigInt(payload.length), 2);
  return Buffer.concat([header, payload]);
}

function decodeClientFrames(buffer) {
  const frames = [];
  let offset = 0;
  while (buffer.length - offset >= 2) {
    const first = buffer[offset];
    const second = buffer[offset + 1];
    let length = second & 0x7f;
    let cursor = offset + 2;
    if (length === 126) {
      if (buffer.length - cursor < 2) break;
      length = buffer.readUInt16BE(cursor);
      cursor += 2;
    } else if (length === 127) {
      if (buffer.length - cursor < 8) break;
      length = Number(buffer.readBigUInt64BE(cursor));
      cursor += 8;
    }
    assert.ok(second & 0x80, "client frames must be masked");
    if (buffer.length - cursor < 4 + length) break;
    const mask = buffer.subarray(cursor, cursor + 4);
    cursor += 4;
    const payload = Buffer.alloc(length);
    for (let index = 0; index < length; index += 1) payload[index] = buffer[cursor + index] ^ mask[index % 4];
    frames.push({ opcode: first & 0x0f, payload });
    offset = cursor + length;
  }
  return { frames, rest: buffer.subarray(offset) };
}

async function fakeDaemon(onMessage, onFrame = () => {}) {
  const directory = await mkdtemp(path.join(os.tmpdir(), "daemon-run-test-"));
  const socketPath = path.join(directory, "daemon.sock");
  const connections = new Set();
  const server = net.createServer((socket) => {
    connections.add(socket);
    socket.on("close", () => connections.delete(socket));
    let upgraded = false;
    let buffer = Buffer.alloc(0);
    socket.on("data", (chunk) => {
      buffer = Buffer.concat([buffer, chunk]);
      if (!upgraded) {
        const end = buffer.indexOf("\r\n\r\n");
        if (end === -1) return;
        const request = buffer.subarray(0, end).toString();
        const key = request.match(/^Sec-WebSocket-Key:\s*(.+)$/im)?.[1].trim();
        assert.ok(key);
        const accept = crypto.createHash("sha1").update(key + websocketGuid).digest("base64");
        socket.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n\r\n`);
        buffer = buffer.subarray(end + 4);
        upgraded = true;
      }
      const decoded = decodeClientFrames(buffer);
      buffer = Buffer.from(decoded.rest);
      for (const frame of decoded.frames) {
        onFrame(frame, socket);
        if (frame.opcode === 1) onMessage(JSON.parse(frame.payload.toString()), socket);
      }
    });
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(socketPath, resolve);
  });
  return {
    socketPath,
    async close() {
      for (const socket of connections) socket.destroy();
      await new Promise((resolve) => server.close(resolve));
      await rm(directory, { recursive: true, force: true });
    }
  };
}

function send(socket, message, options) {
  socket.write(serverFrame(JSON.stringify(message), options));
}

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
      env: { ...process.env, EXECUTE_DAEMON_SOCKET: socketPath },
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
    assert.deepEqual(start, { cwd: path.resolve("plugins"), approvalPolicy: "never", sandbox: "read-only", serviceName: "codex-execute", ephemeral: false });
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
