# ABOUTME: hub-side addon that fans each flow out into the file of every active capture whose
# ABOUTME: host filter and time window it matches, so a `read` touches only that app's data.
import json
import os
import re
from pathlib import Path

from mitmproxy import http, io


def _root() -> Path:
    return Path(os.environ.get("PROXY_DIR") or "/tmp/proxy")


def _captures_dir() -> Path:
    return _root() / "captures"


def _cap_file(cap_id: str) -> Path:
    return _root() / "cap" / f"{cap_id}.mitm"


# cap_id -> (file object, FlowWriter, host regex, started_at)
_writers: dict[str, tuple] = {}
_dir_mtime = -1.0


def _sync() -> None:
    """Open a writer for each new capture record, close writers for removed ones.

    Re-scanned only when the captures directory changes (one stat per flow), so a running
    capture appearing or ending is picked up without globbing on every flow.
    """
    global _dir_mtime
    d = _captures_dir()
    try:
        m = d.stat().st_mtime
    except FileNotFoundError:
        m = 0.0
    if m == _dir_mtime:
        return
    _dir_mtime = m

    active = {}
    for p in d.glob("*.json"):
        try:
            r = json.loads(p.read_text())
        except (json.JSONDecodeError, FileNotFoundError):
            continue
        active[r["id"]] = r

    for cid in list(_writers):
        if cid not in active:
            f, _w, _rx, _s = _writers.pop(cid)
            f.close()

    for cid, r in active.items():
        if cid not in _writers:
            fp = _cap_file(cid)
            fp.parent.mkdir(parents=True, exist_ok=True)
            f = open(fp, "ab")
            _writers[cid] = (f, io.FlowWriter(f), r.get("hostRegex") or "",
                             float(r.get("started_at", 0) or 0))


def _write(flow: http.HTTPFlow) -> None:
    _sync()
    host = flow.request.pretty_host
    ts = flow.request.timestamp_start or 0
    for _cid, (f, writer, rx, started) in _writers.items():
        if ts < started:
            continue
        if rx and not re.search(rx, host):
            continue
        writer.add(flow)
        f.flush()


def response(flow: http.HTTPFlow) -> None:
    # A plain HTTP flow is complete at its response; a WebSocket flow is written at close so
    # its frames are included (see websocket_end).
    if flow.websocket is None:
        _write(flow)


def websocket_end(flow: http.HTTPFlow) -> None:
    _write(flow)


def done() -> None:
    for f, _w, _rx, _s in _writers.values():
        f.close()
    _writers.clear()
