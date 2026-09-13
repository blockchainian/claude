#!/usr/bin/env python3
"""Tests for capture.py's target scoping and per-run isolation.

Covers the two properties the skill leans on: the host filter matches an app's own domains
and subdomains but not lookalikes, and port claiming hands two concurrent runs different
ports while reusing a port whose holder has died.
"""

import importlib.util
import json
import os
import re
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
        self.rx = re.compile(capture.host_regex(["pump.fun", "api.pump.fun"]))

    def test_matches_domain_and_subdomains(self):
        for host in ("pump.fun", "api.pump.fun", "frontend-api-v3.pump.fun", "pump.fun:443"):
            self.assertRegex(host, self.rx, host)

    def test_rejects_lookalikes(self):
        for host in ("notpump.fun", "pump.fund", "pump.funny.com", "evil.com"):
            self.assertNotRegex(host, self.rx, host)

    def test_escapes_dots(self):
        # A dot in a domain must be a literal, not "any char": "pumpXfun" must not match.
        rx = re.compile(capture.host_regex(["pump.fun"]))
        self.assertNotRegex("pumpxfun", rx)


class PortClaim(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self._old = os.environ.get("TMPDIR")
        os.environ["TMPDIR"] = self._tmp.name

    def tearDown(self):
        if self._old is None:
            os.environ.pop("TMPDIR", None)
        else:
            os.environ["TMPDIR"] = self._old
        self._tmp.cleanup()

    def test_two_runs_get_different_ports(self):
        a = capture.claim_port(None, "runA")
        b = capture.claim_port(None, "runB")
        self.assertNotEqual(a, b)
        # Each claim left a lock naming its run.
        self.assertEqual(json.loads(capture.port_lock(a).read_text())["run"], "runA")
        self.assertEqual(json.loads(capture.port_lock(b).read_text())["run"], "runB")

    def test_dead_holder_port_is_reused(self):
        first = capture.claim_port(None, "runA")
        # Rewrite the lock as held by a pid that cannot be alive.
        capture.port_lock(first).write_text(json.dumps(
            {"run": "ghost", "port": first, "pid": 2**31 - 1, "since": 0}))
        reused = capture.claim_port(first, "runB")
        self.assertEqual(reused, first)
        self.assertEqual(json.loads(capture.port_lock(first).read_text())["run"], "runB")


class Human(unittest.TestCase):
    def test_sizes(self):
        self.assertEqual(capture._human(0), "0B")
        self.assertEqual(capture._human(512), "512B")
        self.assertEqual(capture._human(1536), "1.5KB")
        self.assertEqual(capture._human(44 * 1024 * 1024), "44.0MB")


if __name__ == "__main__":
    unittest.main()
