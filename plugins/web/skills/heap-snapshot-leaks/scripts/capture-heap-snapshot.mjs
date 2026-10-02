#!/usr/bin/env node
// ABOUTME: Captures a V8 .heapsnapshot from a running Chrome over the DevTools protocol (debug port).
// ABOUTME: Connects to an existing tab and streams HeapProfiler.takeHeapSnapshot chunks to a file.
//
// Capture a heap snapshot from a Chrome that is already running with a debug port.
//
// Start Chrome with `--remote-debugging-port=9222` (a normal browsing session is
// fine), open the page under test, then run this before and after the repeated
// action. Feed the two files to diff-heap-snapshots.mjs.
//
// Reads and snapshots the chosen tab only; it does not navigate or click. Prints
// JSON describing what it wrote. Use --list to see the open tabs first.
//
// No npm dependencies: the DevTools socket uses the global `WebSocket` and the
// /json endpoints use fetch(), so this script needs Node 22 or newer (the rest
// of the marketplace runs on Node >= 18.18; this one script is the exception).
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, normalize } from "node:path";
import { die, parseArgs, pyFloat, pyJson, pyRepr } from "./argparse-compat.mjs";

const DESCRIPTION = `Capture a heap snapshot from a Chrome that is already running with a debug port.

Start Chrome with \`--remote-debugging-port=9222\` (a normal browsing session is
fine), open the page under test, then run this before and after the repeated
action. Feed the two files to diff-heap-snapshots.mjs.

Reads and snapshots the chosen tab only; it does not navigate or click. Prints
JSON describing what it wrote. Use --list to see the open tabs first.`;

async function targets(port) {
  const url = `http://localhost:${port}/json/list`;
  let list;
  try {
    const resp = await fetch(url, { signal: AbortSignal.timeout(5000) });
    if (!resp.ok) throw new Error(`HTTP Error ${resp.status}: ${resp.statusText}`);
    list = await resp.json();
  } catch (exc) {
    die(`cannot reach Chrome on port ${port}: ${exc?.cause?.message ?? exc.message}. ` +
        `Start Chrome with --remote-debugging-port=${port}.`);
  }
  return list.filter((t) => t.type === "page");
}

async function pick(port, urlContains, index) {
  const pages = await targets(port);
  if (!pages.length) die(`no open page tabs on port ${port}`);
  if (urlContains) {
    const matches = pages.filter((t) => (t.url ?? "").includes(urlContains));
    if (!matches.length) {
      die(`no tab whose URL contains ${pyRepr(urlContains)}; open tabs: ` +
          pages.map((t) => t.url ?? "").join(", "));
    }
    return matches[0];
  }
  if (index >= pages.length) die(`--target-index ${index} out of range (${pages.length} tabs)`);
  return pages[index];
}

function snapshot(wsUrl, deadlineS) {
  if (typeof globalThis.WebSocket === "undefined") {
    die("capture-heap-snapshot.mjs needs the global WebSocket, which requires Node 22 or newer " +
        `(running ${process.version})`);
  }
  // Chrome (v111+) rejects CDP sockets that carry a disallowed Origin header;
  // Node's WebSocket sends none, as Playwright and puppeteer do, so the browser
  // does not need --remote-allow-origins.
  return new Promise((resolve) => {
    const ws = new WebSocket(wsUrl);
    const chunks = [];
    let settled = false;
    const finish = (fn) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      try { ws.close(); } catch { /* already closed */ }
      fn();
    };
    const timer = setTimeout(() => finish(() => die(`timed out after ${pyFloat(deadlineS)}s waiting for the snapshot`)),
                             deadlineS * 1000);
    ws.onopen = () => {
      ws.send(JSON.stringify({ id: 1, method: "HeapProfiler.enable" }));
      ws.send(JSON.stringify({ id: 2, method: "HeapProfiler.collectGarbage" }));
      ws.send(JSON.stringify({ id: 3, method: "HeapProfiler.takeHeapSnapshot",
                               params: { reportProgress: false, captureNumericValue: false } }));
    };
    ws.onmessage = (ev) => {
      const msg = JSON.parse(ev.data);
      if (msg.method === "HeapProfiler.addHeapSnapshotChunk") {
        chunks.push(msg.params.chunk);
      } else if (msg.id === 3) {
        if ("error" in msg) finish(() => die(`takeHeapSnapshot failed: ${pyRepr(msg.error)}`));
        else finish(() => resolve(chunks.join("")));
      }
    };
    ws.onerror = (ev) => finish(() => die(`websocket error connecting to ${wsUrl}: ${ev.message ?? ev.error?.message ?? "connection failed"}`));
    ws.onclose = (ev) => finish(() => die(`websocket closed before the snapshot completed (code ${ev.code})`));
  });
}

// len() of a Python str: code points, not UTF-16 units.
function codePoints(s) {
  return s.length - (s.match(/[\uD800-\uDBFF][\uDC00-\uDFFF]/g)?.length ?? 0);
}

async function main() {
  const args = parseArgs(process.argv.slice(2), {
    description: DESCRIPTION,
    options: [
      { flag: "--out", dest: "out", type: "path", help: "file to write the .heapsnapshot to" },
      { flag: "--port", dest: "port", type: "int", default: 9222, help: "Chrome remote debugging port" },
      { flag: "--url-contains", dest: "urlContains", type: "str", default: null, metavar: "URL_CONTAINS",
        help: "pick the tab whose URL contains this" },
      { flag: "--target-index", dest: "targetIndex", type: "int", default: 0, metavar: "TARGET_INDEX",
        help: "pick the Nth page tab (default 0)" },
      { flag: "--timeout", dest: "timeout", type: "float", default: 120.0, help: "seconds to wait for the snapshot" },
      { flag: "--list", dest: "list", type: "flag", help: "list open page tabs and exit" },
    ],
  });

  if (args.list) {
    const tabs = await targets(args.port);
    console.log(pyJson(tabs.map((t, i) => ({ index: i, title: t.title ?? null, url: t.url ?? null }))));
    return 0;
  }

  if (!args.out) die("--out is required (or pass --list)");
  const out = normalize(args.out).replace(/(.)\/+$/, "$1");
  const target = await pick(args.port, args.urlContains, args.targetIndex);
  const data = await snapshot(target.webSocketDebuggerUrl, args.timeout);
  if (!data) die("snapshot was empty; the tab may have closed mid-capture");
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, data);
  // "bytes" counts code points, as the original len(str) did; the file is written as UTF-8.
  console.log(pyJson({ out, bytes: codePoints(data),
                       target: { title: target.title ?? null, url: target.url ?? null } }));
  return 0;
}

process.exitCode = await main();
