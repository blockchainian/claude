#!/usr/bin/env python3
"""Drive one shared mitmproxy hub, and take app captures as views into its stream.

One long-lived mitmdump — the hub — serves the HTTP proxy (for a Mac browser via Zero Omega)
and, when asked, WireGuard (for a phone) at the same time, on the fixed port 8080. Every
session and agent shares it: the browser needs only one Zero Omega profile, pointed at 8080,
and the phone needs only one tunnel. The hub records everything routed to it into one flow
file, unfiltered — Zero Omega and WireGuard already decide what reaches it.

A capture is not a process; it is a named, time-stamped view into the hub's stream. `start`
notes the moment and the target hosts; the readers then show only that capture's window,
scoped to its hosts (and, for a shared host, its caller). So two agents capturing two apps at
once are two records over one hub, separated at read time — no second proxy, no second port,
no second Zero Omega profile.

Subcommands:
  start   ensure the hub is up and open a capture; prints its id and the 8080 address
  check   is the hub up, and is this capture's traffic arriving?
  read    show this capture's flows / websocket frames / hosts / callers
  stop    close a capture (the hub keeps running); prints its summary
  status  the hub's state and the open captures
  down    stop the hub itself; --wipe also deletes its flow file

Exit 0 on success, 1 on failure, 2 on bad arguments.
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


def hub_dir() -> Path:
    return out_root() / "hub"


def captures_dir() -> Path:
    return out_root() / "captures"


def cap_dir() -> Path:
    return out_root() / "cap"


def cap_file(cap_id: str) -> Path:
    return cap_dir() / f"{cap_id}.mitm"


def hub_meta_path() -> Path:
    return hub_dir() / "meta.json"


def hub_lock() -> Path:
    return Path(os.environ.get("TMPDIR") or "/tmp") / "proxy-hub.json"


HUB_PORT = 8080


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

    Matches the domain itself and any subdomain, with an optional :port, anchored at the end
    so "pump.fun" does not also match "notpump.fun". Saved on a capture and used by the
    readers to scope to that app's hosts.
    """
    alts = "|".join(re.escape(d.strip()) for d in domains if d.strip())
    return rf"(?:^|\.)(?:{alts})(?::\d+)?$"


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


def _human(n: float) -> str:
    for unit in ("B", "KB", "MB", "GB"):
        if n < 1024:
            return f"{n:.0f}{unit}" if unit == "B" else f"{n:.1f}{unit}"
        n /= 1024
    return f"{n:.1f}TB"


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


def hub_running() -> dict | None:
    meta = _read(hub_meta_path())
    if meta and pid_alive(meta.get("pid", -1)):
        return meta
    return None


def ensure_hub(wireguard: bool) -> tuple[dict | None, str | None]:
    """Return (meta, error). Start the hub if it is not already running.

    The hub is one mitmdump serving `regular@8080` and, if asked, `wireguard`, writing every
    flow to the shared file. If it is already up, it is reused as-is; a WireGuard request
    against a hub that has no WireGuard mode is reported so the caller can restart it.
    """
    meta = hub_running()
    if meta:
        if wireguard and "wireguard" not in meta.get("modes", []):
            return meta, "hub is running without WireGuard; run `down` then `start --wireguard`"
        return meta, None

    if _port_in_use(HUB_PORT):
        return None, (f"port {HUB_PORT} is held by another process — free it, then start "
                      "(the hub needs 8080)")

    hub_dir().mkdir(parents=True, exist_ok=True)
    log_file = hub_dir() / "mitmdump.log"
    scripts = Path(__file__).parent
    # connlog.py records the timestamped connection/request log (for a fast `check`);
    # dispatch.py fans each flow out into the file of every active capture it matches.
    modes = ["regular@8080"] + (["wireguard"] if wireguard else [])
    cmd = ["mitmdump", "-q", "-s", str(scripts / "connlog.py"), "-s", str(scripts / "dispatch.py")]
    for m in modes:
        cmd += ["--mode", m]

    with open(log_file, "w") as log:
        proc = subprocess.Popen(cmd, stdout=log, stderr=subprocess.STDOUT,
                                start_new_session=True)
    time.sleep(0.7)
    if not pid_alive(proc.pid) or not _port_in_use(HUB_PORT):
        tail = log_file.read_text()[-500:] if log_file.exists() else ""
        if pid_alive(proc.pid):
            proc.terminate()
        return None, f"hub failed to start: {tail.strip()}"

    meta = {"pid": proc.pid, "since": time.time(), "port": HUB_PORT,
            "modes": ["regular"] + (["wireguard"] if wireguard else []),
            "log": str(log_file)}
    hub_meta_path().write_text(json.dumps(meta, indent=2))
    hub_lock().write_text(json.dumps({"pid": proc.pid, "since": meta["since"]}))
    return meta, None


def cmd_start(args: argparse.Namespace) -> int:
    meta, err = ensure_hub(args.wireguard)
    if err and not meta:
        print(json.dumps({"started": False, "reason": err}))
        return 1

    cap_id = f"{time.strftime('%Y%m%d-%H%M%S')}-{os.getpid()}"
    if args.label:
        cap_id = f"{cap_id}-{re.sub(r'[^A-Za-z0-9_-]', '', args.label)}"
    domains = [d for d in (args.hosts or "").split(",") if d.strip()]
    regex = args.host_regex or (host_regex(domains) if domains else "")
    record = {"id": cap_id, "label": args.label, "started_at": time.time(),
              "hosts": domains, "hostRegex": regex}
    captures_dir().mkdir(parents=True, exist_ok=True)
    (captures_dir() / f"{cap_id}.json").write_text(json.dumps(record, indent=2))

    out: dict[str, object] = {"started": True, "capture": cap_id, "hosts": domains,
                              "proxy": f"{lan_ip()}:{HUB_PORT}", "proxyLocal": f"127.0.0.1:{HUB_PORT}",
                              "hubModes": (meta or {}).get("modes", [])}
    if err:
        out["warning"] = err
    if "wireguard" in (meta or {}).get("modes", []):
        out["wireguardConfigHint"] = "run wg_config.py to emit the client config and QR"
        out["endpoint"] = f"{lan_ip()}:51820"
    if not domains:
        out["note"] = "no --hosts: readers show every host in the capture window; pass --hosts to scope"
    print(json.dumps(out, indent=2))
    return 0


def _capture(cap_id: str) -> dict | None:
    return _read(captures_dir() / f"{cap_id}.json")


def _reader_output(record: dict, addon: str, extra: list[str]) -> str:
    """Run a reader addon over this capture's own file — already scoped by host and window,
    so a read touches only this app's data, not the whole hub."""
    f = cap_file(record["id"])
    if not f.exists():
        return ""
    cmd = ["mitmdump", "-q", "-nr", str(f),
           "-s", str(Path(__file__).with_name(addon))] + extra
    return subprocess.run(cmd, capture_output=True, text=True).stdout


def _log_counts(record: dict) -> tuple[int, int, int]:
    """Count this capture's client connections, requests, and TLS failures from the hub log.

    connlog.py writes a timestamped line per client connection and per request host, so a
    capture's window (start time) and hosts are counted straight from the log — no re-parse of
    the whole shared flow file, whatever its size.
    """
    since = float(record.get("started_at", 0) or 0)
    rx = record.get("hostRegex") or ""
    try:
        text = (hub_dir() / "mitmdump.log").read_text(errors="replace")
    except FileNotFoundError:
        return 0, 0, 0
    clients = requests = tls_failed = 0
    for line in text.splitlines():
        parts = line.split(maxsplit=2)
        if len(parts) >= 2 and parts[0] == "PROXY_CLIENT_CONNECTED":
            try:
                if float(parts[1]) >= since:
                    clients += 1
            except ValueError:
                pass
        elif len(parts) == 3 and parts[0] in ("PROXY_REQUEST", "PROXY_TLS_FAILED"):
            try:
                fresh = float(parts[1]) >= since
            except ValueError:
                continue
            if fresh and (not rx or re.search(rx, parts[2])):
                if parts[0] == "PROXY_REQUEST":
                    requests += 1
                else:
                    tls_failed += 1
    return clients, requests, tls_failed


def cmd_check(args: argparse.Namespace) -> int:
    record = _capture(args.capture)
    if not record:
        print(json.dumps({"ok": False, "reason": "capture not found", "capture": args.capture}))
        return 2
    meta = hub_running()
    if not meta:
        print(json.dumps({"ok": False, "capture": args.capture, "hubRunning": False,
                          "verdict": "the hub is not running — start a capture first"}))
        return 0

    clients, requests, tls_failed = _log_counts(record)

    if clients == 0:
        verdict = ("nothing has connected to the hub — enable Zero Omega for the site (proxy "
                   "8080) or turn on the phone tunnel, then use the app")
    elif requests == 0 and tls_failed > 0:
        verdict = (f"connections are arriving but TLS is failing on {tls_failed} — the client "
                   "does not trust the mitmproxy CA (on the phone: install via mitm.it, then "
                   "Settings > General > About > Certificate Trust Settings), or the app pins "
                   "its certificate")
    elif requests == 0:
        verdict = ("the hub has traffic but none for this capture's hosts since it started — "
                   "widen --hosts, or whatever routes to the hub (Zero Omega for the browser, "
                   "the WireGuard tunnel for the phone) is not sending the app's hosts")
    else:
        verdict = f"capturing: {requests} request(s) to this capture's hosts"
    print(json.dumps({"ok": requests > 0, "capture": args.capture, "hubRunning": True,
                      "clientsConnected": clients, "requests": requests,
                      "tlsFailed": tls_failed, "verdict": verdict}, indent=2))
    return 0


READERS = {"flows": "flowlog.py", "ws": "wslog.py", "hosts": "hosts.py", "origins": "origins.py"}


def cmd_read(args: argparse.Namespace) -> int:
    record = _capture(args.capture)
    if not record:
        print(f"capture not found: {args.capture}", file=sys.stderr)
        return 2
    extra: list[str] = []
    if args.source:
        extra += ["--set", f"source={args.source}"]
    if args.wsmax:
        extra += ["--set", f"wsmax={args.wsmax}"]
    sys.stdout.write(_reader_output(record, READERS[args.kind], extra))
    return 0


def cmd_stop(args: argparse.Namespace) -> int:
    path = captures_dir() / f"{args.capture}.json"
    record = _read(path)
    if not record:
        print(json.dumps({"stopped": False, "reason": "capture not found", "capture": args.capture}))
        return 2
    _, n, _ = _log_counts(record)
    path.unlink(missing_ok=True)
    wiped = False
    if args.wipe:
        cap_file(args.capture).unlink(missing_ok=True)
        wiped = True
    print(json.dumps({"stopped": True, "capture": args.capture, "requests": n, "wiped": wiped,
                      "note": "the hub keeps running; use `down` to stop it"}, indent=2))
    return 0


def cmd_status(_: argparse.Namespace) -> int:
    meta = hub_running()
    captures = []
    for p in sorted(captures_dir().glob("*.json")):
        r = _read(p)
        if r:
            f = cap_file(r["id"])
            size = f.stat().st_size if f.exists() else 0
            captures.append({"capture": r.get("id"), "label": r.get("label"),
                             "hosts": r.get("hosts"), "bytes": size, "human": _human(size)})
    print(json.dumps({
        "hubRunning": meta is not None,
        "hubModes": (meta or {}).get("modes", []),
        "captures": captures,
    }, indent=2))
    return 0


def cmd_down(args: argparse.Namespace) -> int:
    meta = _read(hub_meta_path())
    pid = (meta or {}).get("pid", -1)
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
    hub_meta_path().unlink(missing_ok=True)
    hub_lock().unlink(missing_ok=True)
    wiped = False
    if args.wipe:
        for f in cap_dir().glob("*.mitm"):
            f.unlink()
        for p in captures_dir().glob("*.json"):
            p.unlink()
        wiped = True
    print(json.dumps({"down": True, "wiped": wiped}, indent=2))
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    s = sub.add_parser("start", help="ensure the hub is up and open a capture")
    s.add_argument("--label", help="short name for the capture")
    s.add_argument("--hosts", help="comma-separated target domains to scope the readers to")
    s.add_argument("--host-regex", help="raw host regex, overrides --hosts")
    s.add_argument("--wireguard", action="store_true", help="also serve WireGuard for a phone")
    s.set_defaults(func=cmd_start)

    c = sub.add_parser("check", help="is this capture's traffic arriving?")
    c.add_argument("capture", help="capture id from start")
    c.set_defaults(func=cmd_check)

    r = sub.add_parser("read", help="show this capture's flows/ws/hosts/callers")
    r.add_argument("capture", help="capture id from start")
    r.add_argument("--kind", choices=list(READERS), default="flows")
    r.add_argument("--source", help="origins: pull one caller (Origin/Referer/app-id substring)")
    r.add_argument("--wsmax", type=int, help="ws: max chars per frame")
    r.set_defaults(func=cmd_read)

    p = sub.add_parser("stop", help="close a capture (hub keeps running)")
    p.add_argument("capture", help="capture id from start")
    p.add_argument("--wipe", action="store_true", help="also delete this capture's flow file")
    p.set_defaults(func=cmd_stop)

    st = sub.add_parser("status", help="hub state and open captures")
    st.set_defaults(func=cmd_status)

    d = sub.add_parser("down", help="stop the hub itself")
    d.add_argument("--wipe", action="store_true", help="also delete the hub flow file")
    d.set_defaults(func=cmd_down)

    args = ap.parse_args()
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
