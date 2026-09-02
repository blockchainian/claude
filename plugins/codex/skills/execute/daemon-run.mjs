#!/usr/bin/env node
// ABOUTME: Runs one codex task as a persistent app-server daemon thread.
// ABOUTME: Implements the daemon's WebSocket transport without dependencies.

import crypto from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const usage = "usage: daemon-run.mjs -C <dir> -o <file> --name <name> --timeout <seconds> [-s <sandbox>] <prompt|->";

function connectionError(message) {
  const error = new Error(message);
  error.exitCode = 3;
  return error;
}

function parseArgs(argv) {
  const options = { sandbox: "workspace-write" };
  const values = new Map([["-C", "cwd"], ["-o", "output"], ["--name", "name"], ["--timeout", "timeout"], ["-s", "sandbox"]]);
  const positional = [];
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (values.has(argument)) {
      if (index + 1 >= argv.length) throw new Error("missing option value");
      options[values.get(argument)] = argv[++index];
    } else if (argument.startsWith("-") && argument !== "-") {
      throw new Error(`unknown option: ${argument}`);
    } else {
      positional.push(argument);
    }
  }
  const seconds = Number(options.timeout);
  if (!options.cwd || !options.output || !options.name || positional.length !== 1 || !Number.isFinite(seconds) || seconds <= 0) {
    throw new Error("invalid arguments");
  }
  return { ...options, cwd: path.resolve(options.cwd), timeout: seconds, prompt: positional[0] };
}

function frame(opcode, payload) {
  payload = Buffer.isBuffer(payload) ? payload : Buffer.from(payload);
  const size = payload.length;
  const extended = size < 126 ? 0 : size < 65536 ? 2 : 8;
  const header = Buffer.alloc(2 + extended + 4);
  header[0] = 0x80 | opcode;
  header[1] = 0x80 | (extended === 0 ? size : extended === 2 ? 126 : 127);
  if (extended === 2) header.writeUInt16BE(size, 2);
  if (extended === 8) header.writeBigUInt64BE(BigInt(size), 2);
  const maskOffset = 2 + extended;
  const mask = crypto.randomBytes(4);
  mask.copy(header, maskOffset);
  const masked = Buffer.alloc(size);
  for (let index = 0; index < size; index += 1) masked[index] = payload[index] ^ mask[index % 4];
  return Buffer.concat([header, masked]);
}

class WebSocketClient {
  constructor(socketPath) {
    this.socketPath = socketPath;
    this.buffer = Buffer.alloc(0);
    this.pending = new Map();
    this.nextId = 1;
    this.fragments = [];
  }

  connect() {
    return new Promise((resolve, reject) => {
      const key = crypto.randomBytes(16).toString("base64");
      const expected = crypto.createHash("sha1").update(`${key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11`).digest("base64");
      this.socket = net.createConnection({ path: this.socketPath });
      let headers = Buffer.alloc(0);
      const fail = (error) => reject(connectionError(`cannot connect to daemon: ${error.message}`));
      this.socket.once("error", fail);
      this.socket.once("connect", () => {
        this.socket.write(`GET / HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: ${key}\r\nSec-WebSocket-Version: 13\r\n\r\n`);
      });
      const handshake = (chunk) => {
        headers = Buffer.concat([headers, chunk]);
        const end = headers.indexOf("\r\n\r\n");
        if (end === -1) return;
        const text = headers.subarray(0, end).toString();
        const accepted = text.match(/^Sec-WebSocket-Accept:\s*(.+)$/im)?.[1].trim();
        if (!/^HTTP\/1\.1 101\b/.test(text) || accepted !== expected) {
          this.socket.destroy();
          reject(connectionError("daemon WebSocket handshake failed"));
          return;
        }
        this.socket.off("data", handshake);
        this.socket.off("error", fail);
        this.socket.on("data", (data) => this.consume(data));
        this.socket.on("error", (error) => this.fail(error));
        this.socket.on("close", () => this.fail(new Error("daemon closed the connection")));
        const remainder = headers.subarray(end + 4);
        if (remainder.length) this.consume(remainder);
        resolve();
      };
      this.socket.on("data", handshake);
    });
  }

  send(value) {
    this.socket.write(frame(1, JSON.stringify(value)));
  }

  request(method, params) {
    const id = this.nextId++;
    this.send({ id, method, params });
    return new Promise((resolve, reject) => this.pending.set(id, { resolve, reject }));
  }

  consume(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    while (this.buffer.length >= 2) {
      const first = this.buffer[0];
      const second = this.buffer[1];
      let length = second & 0x7f;
      let offset = 2;
      if (length === 126) {
        if (this.buffer.length < 4) return;
        length = this.buffer.readUInt16BE(2);
        offset = 4;
      } else if (length === 127) {
        if (this.buffer.length < 10) return;
        const largeLength = this.buffer.readBigUInt64BE(2);
        if (largeLength > BigInt(Number.MAX_SAFE_INTEGER)) return this.fail(new Error("WebSocket frame is too large"));
        length = Number(largeLength);
        offset = 10;
      }
      const maskLength = second & 0x80 ? 4 : 0;
      if (this.buffer.length < offset + maskLength + length) return;
      let payload = this.buffer.subarray(offset + maskLength, offset + maskLength + length);
      if (maskLength) {
        const key = this.buffer.subarray(offset, offset + 4);
        payload = Buffer.from(payload, (_, index) => payload[index] ^ key[index % 4]);
      }
      this.buffer = this.buffer.subarray(offset + maskLength + length);
      this.handleFrame(first & 0x0f, Boolean(first & 0x80), payload);
    }
  }

  handleFrame(opcode, finished, payload) {
    if (opcode === 8) {
      this.socket.end(frame(8, payload));
      return;
    }
    if (opcode === 9) {
      this.socket.write(frame(10, payload));
      return;
    }
    if (opcode === 10) return;
    if (opcode === 1) this.fragments = [payload];
    else if (opcode === 0 && this.fragments.length) this.fragments.push(payload);
    else return this.fail(new Error("unexpected WebSocket frame"));
    if (!finished) return;
    const text = Buffer.concat(this.fragments).toString("utf8");
    this.fragments = [];
    let message;
    try { message = JSON.parse(text); } catch { return this.fail(new Error("invalid JSON from daemon")); }
    if (message.id !== undefined && message.method) {
      this.send({ id: message.id, error: { code: -32601, message: "Headless runner cannot answer server requests" } });
    } else if (message.id !== undefined) {
      const pending = this.pending.get(message.id);
      if (!pending) return;
      this.pending.delete(message.id);
      if (message.error) pending.reject(new Error(message.error.message || "daemon request failed"));
      else pending.resolve(message.result);
    } else {
      this.onNotification?.(message);
    }
  }

  fail(error) {
    if (this.failed) return;
    this.failed = true;
    for (const pending of this.pending.values()) pending.reject(error);
    this.pending.clear();
    this.onFailure?.(error);
  }

  close() {
    this.onFailure = undefined;
    if (!this.socket) return;
    if (this.socket.writable) this.socket.end(frame(8, Buffer.alloc(0)));
    this.socket.unref();
  }
}

function completionPromise(client, state) {
  return new Promise((resolve, reject) => {
    client.onFailure = reject;
    client.onNotification = (message) => {
      const { method, params = {} } = message;
      if (params.threadId !== state.threadId) return;
      if (method === "item/started") {
        const item = params.item || {};
        if (item.type === "commandExecution") console.log(`[item commandExecution] ${item.command || ""}`);
        if (item.type === "fileChange") console.log(`[item fileChange] ${(item.changes || []).map((change) => change.path).join(", ")}`);
      }
      if (method === "item/completed" && params.item?.type === "agentMessage") {
        state.lastMessage = params.item.text || "";
        console.log(`[agent] ${state.lastMessage}`);
      }
      if (method === "turn/completed") resolve(params.turn);
    };
  });
}

async function session(client, options, state, version) {
  await client.connect();
  await client.request("initialize", { clientInfo: { title: "codex:execute", name: "codex-execute", version }, capabilities: { experimentalApi: false, requestAttestation: false, optOutNotificationMethods: [] } });
  client.send({ method: "initialized", params: {} });
  const started = await client.request("thread/start", { cwd: options.cwd, approvalPolicy: "never", sandbox: options.sandbox, serviceName: "codex-execute", ephemeral: false });
  state.threadId = started.thread.id;
  console.log(`[thread ${state.threadId}] ${options.name}`);
  await client.request("thread/name/set", { threadId: state.threadId, name: options.name });
  const completed = completionPromise(client, state);
  const turn = await client.request("turn/start", { threadId: state.threadId, input: [{ type: "text", text: options.prompt, text_elements: [] }] });
  state.turnId = turn.turn.id;
  return completed;
}

async function run(options) {
  const pluginPath = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../.claude-plugin/plugin.json");
  const plugin = JSON.parse(await readFile(pluginPath, "utf8"));
  const socketPath = process.env.EXECUTE_DAEMON_SOCKET || path.join(process.env.CODEX_HOME || path.join(os.homedir(), ".codex"), "app-server-control/app-server-control.sock");
  const client = new WebSocketClient(socketPath);
  const state = { threadId: undefined, turnId: undefined, lastMessage: "" };
  let forcedCode;
  let force;
  const forced = new Promise((resolve) => { force = resolve; });
  const stop = (code) => {
    if (forcedCode !== undefined) return;
    forcedCode = code;
    if (state.turnId === undefined) {
      force(undefined);
      return;
    }
    client.request("turn/interrupt", { threadId: state.threadId, turnId: state.turnId }).catch(() => {});
    setTimeout(() => force(undefined), 10_000).unref();
  };
  const timeout = setTimeout(() => stop(124), options.timeout * 1000);
  const onTerm = () => stop(143);
  const onInt = () => stop(130);
  process.once("SIGTERM", onTerm);
  process.once("SIGINT", onInt);
  let result;
  try {
    try {
      result = await Promise.race([session(client, options, state, plugin.version), forced]);
    } catch (error) {
      if (forcedCode !== undefined) return forcedCode;
      throw error;
    }
    if (forcedCode !== undefined) return forcedCode;
    if (result.status === "completed") {
      const finalItem = result.items?.filter((item) => item.type === "agentMessage").at(-1);
      if (finalItem) state.lastMessage = finalItem.text || "";
      await writeFile(options.output, state.lastMessage);
      console.log("[turn] completed");
      return 0;
    }
    if (result.status === "interrupted") return 130;
    console.error(result.error?.message || result.error || "turn failed");
    return 1;
  } finally {
    clearTimeout(timeout);
    process.off("SIGTERM", onTerm);
    process.off("SIGINT", onInt);
    client.close();
  }
}

let options;
try {
  options = parseArgs(process.argv.slice(2));
  if (options.prompt === "-") options.prompt = await new Promise((resolve) => {
    let input = "";
    process.stdin.setEncoding("utf8");
    process.stdin.on("data", (chunk) => { input += chunk; });
    process.stdin.on("end", () => resolve(input));
  });
} catch (error) {
  console.error(`${error.message}\n${usage}`);
  process.exitCode = 2;
}

if (options) {
  try {
    process.exitCode = await run(options);
  } catch (error) {
    console.error(error.message);
    process.exitCode = error.exitCode || 1;
  }
}
