// ABOUTME: Fake codex app-server daemon on a unix socket speaking WebSocket for tests.
// ABOUTME: Decodes masked client frames and lets a script answer JSON-RPC messages.

import assert from "node:assert/strict";
import crypto from "node:crypto";
import { mkdtemp, rm } from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";

const websocketGuid = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

export function serverFrame(payload, { opcode = 1, finished = true } = {}) {
  payload = Buffer.isBuffer(payload) ? payload : Buffer.from(payload);
  const extended = payload.length < 126 ? 0 : payload.length < 65536 ? 2 : 8;
  const header = Buffer.alloc(2 + extended);
  header[0] = (finished ? 0x80 : 0) | opcode;
  header[1] = extended === 0 ? payload.length : extended === 2 ? 126 : 127;
  if (extended === 2) header.writeUInt16BE(payload.length, 2);
  if (extended === 8) header.writeBigUInt64BE(BigInt(payload.length), 2);
  return Buffer.concat([header, payload]);
}

export function decodeClientFrames(buffer) {
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

export async function fakeDaemon(onMessage, onFrame = () => {}, { socketPath: reuse } = {}) {
  const directory = reuse ? undefined : await mkdtemp(path.join(os.tmpdir(), "fake-daemon-"));
  const socketPath = reuse ?? path.join(directory, "daemon.sock");
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
      if (directory) await rm(directory, { recursive: true, force: true });
    }
  };
}

export function send(socket, message, options) {
  socket.write(serverFrame(JSON.stringify(message), options));
}
