// ABOUTME: JSON-RPC client for the codex app-server daemon over its unix WebSocket transport.
// ABOUTME: Implements framing without dependencies and never writes to stdout.

import crypto from "node:crypto";
import net from "node:net";
import os from "node:os";
import path from "node:path";

export function daemonSocketPath(override) {
  return override || path.join(process.env.CODEX_HOME || path.join(os.homedir(), ".codex"), "app-server-control/app-server-control.sock");
}

export function connectionError(message) {
  const error = new Error(message);
  error.exitCode = 3;
  return error;
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

export class WebSocketClient {
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

  /** Server-to-client requests are rejected unless a caller installs a handler. */
  onServerRequest(message) {
    this.send({ id: message.id, error: { code: -32601, message: "Client cannot answer server requests" } });
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
      this.onServerRequest(message);
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
