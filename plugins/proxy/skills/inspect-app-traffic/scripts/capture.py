#!/usr/bin/env python3
"""Run one target-scoped mitmproxy capture, isolated so many can run at once.

Each capture is a run: its own listen port, its own output directory, its own mitmdump
process. Two sessions or agents can capture different apps side by side without touching
each other's ports or files. The only shared thing is the mitmproxy CA in ~/.mitmproxy,
which every run reuses — that is the whole point of installing it once.

Target scoping is the reason to use this over raw mitmdump. Given a list of the target
app's domains, only those hosts are decrypted (--allow-hosts) and only their flows are
written (save_stream_filter). Everything else on the machine or phone — a bank, iMessage,
another app — passes through as an opaque tunnel and is never saved. That keeps the capture
about the one app and keeps unrelated private traffic out of the file.

Subcommands:
  start   launch a capture; prints the connection info and where flows are written
  stop    stop a running capture and report the final flow file and its size
  status  list this user's captures and whether each is still running

Ports and the single WireGuard slot are held with lock files under TMPDIR, so a second
start never lands on a port or the UDP slot another run already took. Exit 0 on success,
2 on bad arguments, 3 when a needed resource (the WireGuard slot) is already held.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path


def out_root() -> Path:
    return Path(os.environ.get("PROXY_DIR") or "/tmp/proxy")


def tmp() -> Path:
    return Path(os.environ.get("TMPDIR") or "/tmp")


def port_lock(port: int) -> Path:
    return tmp() / f"proxy-port.{port}.json"


def wg_lock() -> Path:
    return tmp() / "proxy-wireguard.json"


def pid_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def host_regex(domains: list[str]) -> str:
    """Build a host-matching regex from plain domains.

    Matches the domain itself and any subdomain, with an optional :port, anchored at the
    end so "pump.fun" does not also match "notpump.fun". Used for both --allow-hosts and,
    via ~d, the save filter.
    """
    alts = "|".join(re.escape(d.strip()) for d in domains if d.strip())
    return rf"(?:^|\.)(?:{alts})(?::\d+)?$"


def claim_port(preferred: int | None, run_id: str) -> int:
    """Atomically claim a free port for this run, returning it.

    Defaults to 8080 — mitmproxy's conventional port, which a Zero Omega profile is normally
    pinned to — then falls back to 9081-9279 when 8080 is taken (a local dev server, or a
    second concurrent capture), so parallel captures still each get their own port. 8081 is
    left out of the fallback: a Metro/Expo bundler on 8081 was a real conflict. A port is free
    when nothing listens on it and no live lock holds it.
    """
    candidates = ([preferred] if preferred else [8080]) + [
        p for p in range(9081, 9280)
    ]
    for port in candidates:
        lp = port_lock(port)
        existing = _read(lp)
        if existing and existing.get("run") != run_id and pid_alive(existing.get("pid", -1)):
            continue
        if _port_in_use(port):
            continue
        if existing:
            # A stale lock (dead holder) or our own: clear it so the atomic create below
            # can recreate it. A live foreign holder was already skipped above.
            lp.unlink(missing_ok=True)
        try:
            fd = os.open(lp, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
        except FileExistsError:
            continue
        with os.fdopen(fd, "w") as f:
            json.dump({"run": run_id, "port": port, "pid": os.getpid(), "since": time.time()}, f)
        return port
    raise SystemExit("no free port found in 9080-9279")


def _port_in_use(port: int) -> bool:
    """True if anything already holds this port, mirroring how mitmdump binds.

    Two checks: a connect (catches a listener on any address), and a wildcard bind with
    SO_REUSEADDR — the same options mitmdump uses. Binding the wildcard address detects a
    0.0.0.0 listener such as a running mitmweb (which a bind to 127.0.0.1 misses), while
    SO_REUSEADDR keeps a port in TIME_WAIT — e.g. just after killing the previous holder —
    from reading as in-use, since mitmdump can bind it.
    """
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as c:
        c.settimeout(0.3)
        if c.connect_ex(("127.0.0.1", port)) == 0:
            return True
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            s.bind(("", port))
        except OSError:
            return True
    return False


def _read(path: Path) -> dict | None:
    try:
        return json.loads(path.read_text())
    except (FileNotFoundError, json.JSONDecodeError):
        return None


def lan_ip() -> str:
    try:
        iface = subprocess.check_output(["route", "-n", "get", "default"], text=True)
        m = re.search(r"interface:\s*(\S+)", iface)
        if m:
            ip = subprocess.check_output(["ipconfig", "getifaddr", m.group(1)], text=True).strip()
            if ip:
                return ip
    except (subprocess.CalledProcessError, FileNotFoundError):
        pass
    return "<your-mac-lan-ip>"


def cmd_start(args: argparse.Namespace) -> int:
    run_id = os.environ.get("PROXY_RUN_ID") or f"{time.strftime('%Y%m%d-%H%M%S')}-{os.getpid()}"
    if args.label:
        run_id = f"{run_id}-{re.sub(r'[^A-Za-z0-9_-]', '', args.label)}"
    rundir = out_root() / run_id
    rundir.mkdir(parents=True, exist_ok=True)
    flow_file = rundir / "flows.mitm"
    log_file = rundir / "mitmdump.log"

    domains = [d for d in (args.hosts or "").split(",") if d.strip()]
    regex = args.host_regex or (host_regex(domains) if domains else None)

    connlog = Path(__file__).with_name("connlog.py")
    cmd = ["mitmdump", "-q", "-s", str(connlog), "--set", f"save_stream_file={flow_file}"]
    if regex:
        # allow_hosts is a plain regex; save_stream_filter is mitmproxy's filter language,
        # where | and () are operators — the regex must be quoted so they stay literal.
        cmd += ["--allow-hosts", regex, "--set", f'save_stream_filter=~d "{regex}"']
    # Without a filter we capture everything: this is discovery mode, used to find which
    # hosts an app talks to before scoping to them. Warn, because the file will hold
    # unrelated traffic.

    wg_held = None
    if args.mode == "wireguard":
        lp = wg_lock()
        held = _read(lp)
        if held and pid_alive(held.get("pid", -1)) and held.get("run") != run_id:
            print(json.dumps({"started": False, "reason": "wireguard-slot-held",
                              "heldBy": held.get("run")}))
            print(f"WireGuard slot (UDP 51820) is held by run {held.get('run')}. "
                  f"Only one WireGuard capture runs at a time; stop it or use --mode proxy.",
                  file=sys.stderr)
            return 3
        cmd += ["--mode", "wireguard"]
        wg_held = lp
        port = 51820
    else:
        port = claim_port(args.port, run_id)
        cmd += ["--mode", "regular", "--listen-port", str(port)]

    with open(log_file, "w") as log:
        proc = subprocess.Popen(cmd, stdout=log, stderr=subprocess.STDOUT,
                                start_new_session=True)

    if wg_held is not None:
        wg_held.write_text(json.dumps({"run": run_id, "pid": proc.pid, "since": time.time()}))

    meta = {
        "run": run_id, "mode": args.mode, "pid": proc.pid, "port": port,
        "hosts": domains, "hostRegex": regex, "flowFile": str(flow_file),
        "log": str(log_file), "since": time.time(),
    }
    (rundir / "meta.json").write_text(json.dumps(meta, indent=2))

    # Give mitmdump a moment to fail loudly (bad option, port race) rather than reporting a
    # dead capture as live.
    time.sleep(0.6)
    if not pid_alive(proc.pid):
        tail = log_file.read_text()[-500:] if log_file.exists() else ""
        print(json.dumps({"started": False, "reason": "mitmdump exited", "log": tail}))
        _release(port, wg_held, run_id)
        return 1

    # Confirm the port is actually being served — a capture that lost a bind race can stay
    # alive without listening, and would otherwise be reported as started.
    if args.mode == "proxy" and not _port_in_use(port):
        proc.terminate()
        print(json.dumps({"started": False, "reason": f"nothing listening on {port} "
                          "after launch — the port may be held by another process"}))
        _release(port, wg_held, run_id)
        return 1

    out: dict[str, object] = dict(meta, started=True)
    if args.mode == "wireguard":
        out["wireguardConfigHint"] = "run wg_config.py to emit the client config and QR"
        out["endpoint"] = f"{lan_ip()}:51820"
    else:
        out["proxy"] = f"{lan_ip()}:{port}"
        out["proxyLocal"] = f"127.0.0.1:{port}"
    if not regex:
        out["warning"] = "no host filter: capturing ALL traffic (discovery mode)"
    print(json.dumps(out, indent=2))
    return 0


def _release(port: int, wg_held: Path | None, run_id: str | None) -> None:
    lp = port_lock(port)
    held = _read(lp)
    if held and held.get("run") == run_id:
        lp.unlink(missing_ok=True)
    if wg_held is not None:
        held = _read(wg_held)
        if held and held.get("run") == run_id:
            wg_held.unlink(missing_ok=True)


def _find_meta(run: str) -> Path | None:
    direct = out_root() / run / "meta.json"
    if direct.exists():
        return direct
    for m in out_root().glob("*/meta.json"):
        data = _read(m)
        if data and data.get("run") == run:
            return m
    return None


def cmd_stop(args: argparse.Namespace) -> int:
    meta_path = _find_meta(args.run)
    if not meta_path:
        print(json.dumps({"stopped": False, "reason": "run not found", "run": args.run}))
        return 2
    meta = _read(meta_path) or {}
    pid = meta.get("pid", -1)
    if pid_alive(pid):
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        for _ in range(30):
            if not pid_alive(pid):
                break
            time.sleep(0.1)
        if pid_alive(pid):
            os.kill(pid, signal.SIGKILL)
    _release(meta.get("port", -1), wg_lock() if meta.get("mode") == "wireguard" else None,
             meta.get("run"))
    flow = Path(meta.get("flowFile", ""))
    size = flow.stat().st_size if flow.exists() else 0
    print(json.dumps({"stopped": True, "run": meta.get("run"), "flowFile": str(flow),
                      "flowBytes": size, "flowHuman": _human(size)}, indent=2))
    return 0


def cmd_status(_: argparse.Namespace) -> int:
    runs = []
    for m in sorted(out_root().glob("*/meta.json")):
        meta = _read(m)
        if not meta:
            continue
        flow = Path(meta.get("flowFile", ""))
        runs.append({
            "run": meta.get("run"), "mode": meta.get("mode"), "port": meta.get("port"),
            "hosts": meta.get("hosts"), "running": pid_alive(meta.get("pid", -1)),
            "flowBytes": flow.stat().st_size if flow.exists() else 0,
            "flowFile": str(flow),
        })
    print(json.dumps({"captures": runs}, indent=2))
    return 0


def cmd_check(args: argparse.Namespace) -> int:
    """Report whether the app's traffic is actually reaching the proxy.

    proxy cannot read Zero Omega's on/off state, but it sees what connects to it. The
    capture logs a line when a client connects and when a target-host request is intercepted
    (connlog.py); counting those in the run log tells the three cases apart: nothing
    connected (proxy not enabled), connected but no target requests (rule misses the host),
    or target requests flowing (working).
    """
    meta_path = _find_meta(args.run)
    if not meta_path:
        print(json.dumps({"ok": False, "reason": "run not found", "run": args.run}))
        return 2
    meta = _read(meta_path) or {}
    log = Path(meta.get("log", ""))
    text = log.read_text(errors="replace") if log.exists() else ""
    clients = text.count("PROXY_CLIENT_CONNECTED")
    requests = text.count("PROXY_REQUEST ")
    flow = Path(meta.get("flowFile", ""))
    size = flow.stat().st_size if flow.exists() else 0
    running = pid_alive(meta.get("pid", -1))

    if not running:
        verdict = "capture is not running — start it first"
    elif clients == 0:
        verdict = ("no client has connected to the proxy — the proxy is not enabled "
                   "(enable Zero Omega for the site, or connect the phone), or the app has "
                   "not been used yet")
    elif requests == 0:
        verdict = ("proxy is connected but no target-host requests seen — check the Zero "
                   "Omega rule covers the app's hosts, then use the app")
    else:
        verdict = f"capturing: {requests} request(s) to the target hosts"

    print(json.dumps({
        "ok": requests > 0, "run": meta.get("run"), "running": running,
        "hosts": meta.get("hosts"), "clientsConnected": clients,
        "targetRequests": requests, "flowBytes": size, "flowHuman": _human(size),
        "verdict": verdict,
    }, indent=2))
    return 0


def _human(n: float) -> str:
    for unit in ("B", "KB", "MB", "GB"):
        if n < 1024:
            return f"{n:.0f}{unit}" if unit == "B" else f"{n:.1f}{unit}"
        n /= 1024
    return f"{n:.1f}TB"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    s = sub.add_parser("start", help="launch a capture")
    s.add_argument("--mode", choices=["proxy", "wireguard"], default="proxy",
                   help="proxy: HTTP proxy for a Mac website via Zero Omega. "
                        "wireguard: whole-iPhone capture over a VPN tunnel.")
    s.add_argument("--hosts", help="comma-separated target domains, e.g. pump.fun,api.pump.fun. "
                                   "Omit for discovery mode (captures everything).")
    s.add_argument("--host-regex", help="raw mitmproxy host regex, overrides --hosts")
    s.add_argument("--port", type=int, help="preferred proxy port (proxy mode only)")
    s.add_argument("--label", help="short suffix added to the run id, for your own labelling")
    s.set_defaults(func=cmd_start)

    p = sub.add_parser("stop", help="stop a capture")
    p.add_argument("run", help="run id (or its label suffix as printed by status)")
    p.set_defaults(func=cmd_stop)

    st = sub.add_parser("status", help="list captures")
    st.set_defaults(func=cmd_status)

    c = sub.add_parser("check", help="is the app's traffic reaching the proxy?")
    c.add_argument("run", help="run id (or its label suffix as printed by status)")
    c.set_defaults(func=cmd_check)

    args = ap.parse_args()
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
