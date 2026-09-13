#!/usr/bin/env python3
"""Regression tests for the simulator branch of discover_ios_setup.py.

The report is what an agent pastes into session_set_defaults and simctl, so it
must name the simulator fully and must say the right thing when the requested
one is not booted.
Run: ./test_discover_ios_setup.py
"""

from __future__ import annotations

import importlib.util
from pathlib import Path

BOOTED = [
    {"udid": "AAAA", "name": "iPhone 16 Pro", "osVersion": "18.3", "state": "Booted"},
    {"udid": "BBBB", "name": "iPhone 16", "osVersion": "18.3", "state": "Booted"},
]


def load_module():
    path = Path(__file__).with_name("discover_ios_setup.py")
    spec = importlib.util.spec_from_file_location("discover_ios_setup", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main() -> int:
    mod = load_module()
    mod.list_simulators = lambda: list(BOOTED)
    failures = []

    r = mod.simulator_report("AAAA")
    if r["sessionDefaults"] != {"simulatorId": "AAAA", "simulatorName": "iPhone 16 Pro"}:
        failures.append(f"defaults omit the name, so a stale one survives: {r['sessionDefaults']}")

    r = mod.simulator_report("CCCC")
    if r["ready"] or "CCCC" not in r["missing"][0] or "not booted" not in r["missing"][0]:
        failures.append(f"a shutdown UDID is not reported as such: {r.get('missing')}")

    r = mod.simulator_report(None)
    if r["ready"] or "--device" not in r["missing"][0]:
        failures.append(f"two booted and none requested must ask for --device: {r.get('missing')}")

    mod.list_simulators = lambda: BOOTED[:1]
    r = mod.simulator_report(None)
    if not r["ready"] or r["selectedSimulator"]["udid"] != "AAAA":
        failures.append(f"a single booted simulator was not selected: {r}")

    for f in failures:
        print("FAIL:", f)
    print("PASS: the simulator report names its pick and explains every refusal"
          if not failures else f"{len(failures)} failure(s)")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
