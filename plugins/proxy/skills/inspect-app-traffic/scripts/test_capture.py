#!/usr/bin/env python3
"""Tests for capture.py: host scoping, the port check, and the hub/capture orchestration.

The hub lifecycle and the reader scoping (since + host) are exercised live in a real capture;
here we cover the pure logic and the orchestration that does not need a running mitmdump —
by faking a hub whose pid is this test process, so `start`/`stop`/`status` act on capture
records without launching anything.
"""

import argparse
import contextlib
import importlib.util
import io
import json
import os
import socket
import tempfile
import unittest
from pathlib import Path

_spec = importlib.util.spec_from_file_location(
    "capture", Path(__file__).with_name("capture.py"))
assert _spec and _spec.loader
capture = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(capture)


class HostRegex(unittest.TestCase):
    def setUp(self):
        import re
        self.rx = re.compile(capture.host_regex(["pump.fun", "api.pump.fun"]))

    def test_matches_domain_and_subdomains(self):
        for host in ("pump.fun", "api.pump.fun", "frontend-api-v3.pump.fun", "pump.fun:443"):
            self.assertRegex(host, self.rx, host)

    def test_rejects_lookalikes(self):
        for host in ("notpump.fun", "pump.fund", "pump.funny.com", "evil.com"):
            self.assertNotRegex(host, self.rx, host)

    def test_escapes_dots(self):
        import re
        rx = re.compile(capture.host_regex(["pump.fun"]))
        self.assertNotRegex("pumpxfun", rx)


class PortInUse(unittest.TestCase):
    def test_detects_wildcard_listener(self):
        # A listener on all interfaces (0.0.0.0) must read as in-use — the case a
        # SO_REUSEADDR bind to 127.0.0.1 missed, letting a capture claim an occupied port.
        srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        srv.bind(("", 0))
        srv.listen()
        port = srv.getsockname()[1]
        try:
            self.assertTrue(capture._port_in_use(port))
        finally:
            srv.close()

    def test_unbound_port_reads_free(self):
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.bind(("", 0))
        port = s.getsockname()[1]
        s.close()
        self.assertFalse(capture._port_in_use(port))


class HubCaptures(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self._env = {}
        for k, v in {"PROXY_DIR": self._tmp.name, "TMPDIR": self._tmp.name}.items():
            self._env[k] = os.environ.get(k)
            os.environ[k] = v

    def tearDown(self):
        for k, v in self._env.items():
            if v is None:
                os.environ.pop(k, None)
            else:
                os.environ[k] = v
        self._tmp.cleanup()

    def _fake_hub(self, alive: bool = True):
        # A hub whose pid is this process (alive) makes ensure_hub reuse it without spawning.
        capture.hub_dir().mkdir(parents=True, exist_ok=True)
        pid = os.getpid() if alive else 2**31 - 1
        capture.hub_meta_path().write_text(json.dumps(
            {"pid": pid, "since": 0, "port": 8080, "modes": ["regular"],
             "log": str(capture.hub_dir() / "mitmdump.log")}))

    def _run(self, ns) -> dict:
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            ns.func(ns)
        return json.loads(buf.getvalue())

    def test_start_creates_capture_record(self):
        self._fake_hub()
        out = self._run(argparse.Namespace(func=capture.cmd_start, label="appA",
                                           hosts="pump.fun,api.pump.fun", host_regex=None,
                                           wireguard=False))
        self.assertTrue(out["started"])
        self.assertIn("appA", out["capture"])
        self.assertEqual(out["proxyLocal"], "127.0.0.1:8080")
        rec = json.loads((capture.captures_dir() / f"{out['capture']}.json").read_text())
        self.assertEqual(rec["hosts"], ["pump.fun", "api.pump.fun"])
        self.assertIn("pump", rec["hostRegex"])
        self.assertGreater(rec["started_at"], 0)

    def test_start_without_hub_and_port_busy_fails(self):
        # No hub meta, and 8080 held by a real listener -> start reports failure, no record.
        srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            srv.bind(("", 8080))
            srv.listen()
        except OSError:
            self.skipTest("port 8080 not bindable in this environment")
        try:
            out = self._run(argparse.Namespace(func=capture.cmd_start, label="x", hosts=None,
                                               host_regex=None, wireguard=False))
            self.assertFalse(out["started"])
            self.assertEqual(list(capture.captures_dir().glob("*.json")), [])
        finally:
            srv.close()

    def test_stop_removes_record(self):
        self._fake_hub()
        started = self._run(argparse.Namespace(func=capture.cmd_start, label="b", hosts=None,
                                               host_regex=None, wireguard=False))
        cap = started["capture"]
        out = self._run(argparse.Namespace(func=capture.cmd_stop, capture=cap, wipe=False))
        self.assertTrue(out["stopped"])
        self.assertFalse((capture.captures_dir() / f"{cap}.json").exists())

    def test_stop_wipe_deletes_flow_file(self):
        self._fake_hub()
        started = self._run(argparse.Namespace(func=capture.cmd_start, label="w", hosts=None,
                                               host_regex=None, wireguard=False))
        cap = started["capture"]
        f = capture.cap_file(cap)
        f.parent.mkdir(parents=True, exist_ok=True)
        f.write_bytes(b"flowdata")
        out = self._run(argparse.Namespace(func=capture.cmd_stop, capture=cap, wipe=True))
        self.assertTrue(out["wiped"])
        self.assertFalse(f.exists())

    def test_status_lists_open_captures(self):
        self._fake_hub()
        self._run(argparse.Namespace(func=capture.cmd_start, label="c", hosts="x.com",
                                     host_regex=None, wireguard=False))
        out = self._run(argparse.Namespace(func=capture.cmd_status))
        self.assertTrue(out["hubRunning"])
        self.assertEqual(len(out["captures"]), 1)

    def test_check_missing_capture(self):
        out = self._run(argparse.Namespace(func=capture.cmd_check, capture="nope"))
        self.assertFalse(out["ok"])

    def test_check_counts_from_log_by_since_and_host(self):
        self._fake_hub()
        t0 = 1_000_000.0
        cap = "20260101-000000-1-x"
        capture.captures_dir().mkdir(parents=True, exist_ok=True)
        (capture.captures_dir() / f"{cap}.json").write_text(json.dumps(
            {"id": cap, "label": "x", "started_at": t0, "hosts": ["api.foo.com"],
             "hostRegex": capture.host_regex(["api.foo.com"])}))
        (capture.hub_dir() / "mitmdump.log").write_text(
            f"PROXY_CLIENT_CONNECTED {t0 - 5:.3f}\n"       # before start -> not counted
            f"PROXY_CLIENT_CONNECTED {t0 + 1:.3f}\n"       # after -> counted
            f"PROXY_REQUEST {t0 - 1:.3f} api.foo.com\n"    # before start -> no
            f"PROXY_REQUEST {t0 + 1:.3f} api.foo.com\n"    # after + host match -> yes
            f"PROXY_REQUEST {t0 + 2:.3f} other.com\n")     # after but wrong host -> no
        out = self._run(argparse.Namespace(func=capture.cmd_check, capture=cap))
        self.assertTrue(out["ok"])
        self.assertEqual(out["clientsConnected"], 1)
        self.assertEqual(out["requests"], 1)

    def test_down_clears_a_dead_hub(self):
        self._fake_hub(alive=False)
        out = self._run(argparse.Namespace(func=capture.cmd_down, wipe=False))
        self.assertTrue(out["down"])
        self.assertFalse(capture.hub_meta_path().exists())


if __name__ == "__main__":
    unittest.main()
