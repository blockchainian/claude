#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["websocket-client"]
# ///
# ABOUTME: Captures a V8 .heapsnapshot from a running Chrome over the DevTools protocol (debug port).
# ABOUTME: Connects to an existing tab and streams HeapProfiler.takeHeapSnapshot chunks to a file.
"""Capture a heap snapshot from a Chrome that is already running with a debug port.

Start Chrome with `--remote-debugging-port=9222` (a normal browsing session is
fine), open the page under test, then run this before and after the repeated
action. Feed the two files to diff_heap_snapshots.py.

Reads and snapshots the chosen tab only; it does not navigate or click. Prints
JSON describing what it wrote. Use --list to see the open tabs first.
"""

from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.request
from pathlib import Path

from websocket import create_connection


def targets(port: int) -> list[dict]:
    url = f"http://localhost:{port}/json/list"
    try:
        with urllib.request.urlopen(url, timeout=5) as resp:
            return [t for t in json.loads(resp.read()) if t.get("type") == "page"]
    except OSError as exc:
        sys.exit(f"cannot reach Chrome on port {port}: {exc}. "
                 f"Start Chrome with --remote-debugging-port={port}.")


def pick(port: int, url_contains: str | None, index: int) -> dict:
    pages = targets(port)
    if not pages:
        sys.exit(f"no open page tabs on port {port}")
    if url_contains:
        matches = [t for t in pages if url_contains in t.get("url", "")]
        if not matches:
            sys.exit(f"no tab whose URL contains {url_contains!r}; open tabs: "
                     + ", ".join(t.get("url", "") for t in pages))
        return matches[0]
    if index >= len(pages):
        sys.exit(f"--target-index {index} out of range ({len(pages)} tabs)")
    return pages[index]


def snapshot(ws_url: str, deadline_s: float) -> str:
    # Chrome (v111+) rejects CDP sockets that carry a disallowed Origin header;
    # sending none, as Playwright and puppeteer do, avoids needing
    # --remote-allow-origins on the browser.
    ws = create_connection(ws_url, timeout=deadline_s, max_size=None, suppress_origin=True)
    chunks: list[str] = []
    try:
        ws.send(json.dumps({"id": 1, "method": "HeapProfiler.enable"}))
        ws.send(json.dumps({"id": 2, "method": "HeapProfiler.collectGarbage"}))
        ws.send(json.dumps({"id": 3, "method": "HeapProfiler.takeHeapSnapshot",
                            "params": {"reportProgress": False, "captureNumericValue": False}}))
        end = time.time() + deadline_s
        while time.time() < end:
            msg = json.loads(ws.recv())
            if msg.get("method") == "HeapProfiler.addHeapSnapshotChunk":
                chunks.append(msg["params"]["chunk"])
            elif msg.get("id") == 3:
                if "error" in msg:
                    sys.exit(f"takeHeapSnapshot failed: {msg['error']}")
                break
        else:
            sys.exit(f"timed out after {deadline_s}s waiting for the snapshot")
    finally:
        ws.close()
    return "".join(chunks)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--out", type=Path, help="file to write the .heapsnapshot to")
    ap.add_argument("--port", type=int, default=9222, help="Chrome remote debugging port")
    ap.add_argument("--url-contains", default=None, help="pick the tab whose URL contains this")
    ap.add_argument("--target-index", type=int, default=0, help="pick the Nth page tab (default 0)")
    ap.add_argument("--timeout", type=float, default=120.0, help="seconds to wait for the snapshot")
    ap.add_argument("--list", action="store_true", help="list open page tabs and exit")
    args = ap.parse_args()

    if args.list:
        print(json.dumps([{"index": i, "title": t.get("title"), "url": t.get("url")}
                          for i, t in enumerate(targets(args.port))], indent=2))
        return 0

    if not args.out:
        sys.exit("--out is required (or pass --list)")
    target = pick(args.port, args.url_contains, args.target_index)
    data = snapshot(target["webSocketDebuggerUrl"], args.timeout)
    if not data:
        sys.exit("snapshot was empty; the tab may have closed mid-capture")
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(data)
    print(json.dumps({"out": str(args.out), "bytes": len(data),
                      "target": {"title": target.get("title"), "url": target.get("url")}}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
