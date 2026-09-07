#!/usr/bin/env python3
"""Regression tests for claim_simulator.py and capture_slice.sh.

One agent per simulator: a second claim on a held simulator must fail with a
clear, distinct exit code and say who holds it, and a capture must refuse to
run against a simulator nobody has claimed or against "booted".
Run: ./test_claim_simulator.py
"""

from __future__ import annotations

import json
import os
import subprocess
import tempfile
from pathlib import Path

HERE = Path(__file__).parent
UDID = "00000000-0000-0000-0000-00000000TEST"


def run(*args: str, tmp: str) -> subprocess.CompletedProcess:
    return subprocess.run([str(HERE / args[0]), *args[1:]], capture_output=True,
                          text=True, env={**os.environ, "TMPDIR": tmp}, check=False)


def main() -> int:
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        r = run("claim_simulator.py", UDID, "--run", "run-a", tmp=tmp)
        if r.returncode != 0 or not json.loads(r.stdout)["claimed"]:
            failures.append(f"first claim failed: {r.returncode} {r.stderr}")

        r = run("claim_simulator.py", UDID, "--run", "run-a", tmp=tmp)
        if r.returncode != 0:
            failures.append(f"re-claiming by the same run is not idempotent: {r.stderr}")

        r = run("claim_simulator.py", UDID, "--run", "run-b", tmp=tmp)
        j = json.loads(r.stdout) if r.stdout else {}
        if r.returncode != 3 or j.get("claimed") or j.get("heldBy") != "run-a":
            failures.append(f"second claim was not refused as held: {r.returncode} {r.stdout}")
        if "run-a" not in r.stderr or "wait" not in r.stderr.lower():
            failures.append(f"refusal does not say who holds it or what to do: {r.stderr!r}")

        r = run("claim_simulator.py", UDID, "--run", "run-b", "--release", tmp=tmp)
        if r.returncode != 3:
            failures.append(f"another run could release the lock: {r.returncode}")

        r = run("claim_simulator.py", "booted", "--run", "run-a", tmp=tmp)
        if r.returncode != 2:
            failures.append(f'"booted" was accepted as a UDID: {r.returncode}')

        r = run("capture_slice.sh", "--simulator", "booted", "--out", f"{tmp}/x.png", tmp=tmp)
        if r.returncode != 2:
            failures.append(f'capture accepted "booted": {r.returncode} {r.stderr}')

        r = run("claim_simulator.py", UDID, "--run", "run-b", "--steal", tmp=tmp)
        if r.returncode != 0 or json.loads(r.stdout)["heldBy"] != "run-b":
            failures.append(f"steal did not take the lock: {r.returncode} {r.stdout}")

        r = run("claim_simulator.py", UDID, "--run", "run-b", "--release", tmp=tmp)
        if r.returncode != 0:
            failures.append(f"holder could not release: {r.returncode} {r.stderr}")

        r = run("capture_slice.sh", "--simulator", UDID, "--out", f"{tmp}/x.png", tmp=tmp)
        if r.returncode != 3 or "claim" not in r.stderr:
            failures.append(f"capture ran against an unclaimed simulator: {r.returncode} {r.stderr!r}")

        r = run("claim_simulator.py", UDID, "--run", "run-b", "--release", tmp=tmp)
        if r.returncode != 0:
            failures.append(f"releasing an unheld lock is not a no-op: {r.returncode}")

    for f in failures:
        print("FAIL:", f)
    print("PASS: one run holds a simulator at a time, and capture needs the claim"
          if not failures else f"{len(failures)} failure(s)")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
