#!/usr/bin/env python3
"""Regression tests for claim_simulator.py and capture_slice.sh.

One agent per simulator or phone: a second claim on a held UDID must fail with a
clear, distinct exit code and say who holds it without calling a phone a
simulator, and a capture must refuse to run against a simulator nobody has
claimed or against "booted".
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
PHONE = "00008030-001A2B3C4D5E6F7A"


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

        r = run("capture_slice.sh", "--simulator", "booted", "--run", "run-a",
                "--out", f"{tmp}/x.png", tmp=tmp)
        if r.returncode != 2:
            failures.append(f'capture accepted "booted": {r.returncode} {r.stderr}')

        r = run("capture_slice.sh", "--simulator", UDID, "--run", "run-b",
                "--out", f"{tmp}/x.png", tmp=tmp)
        if r.returncode != 3 or "run-a" not in r.stderr:
            failures.append(f"capture by a run that does not hold the claim was not refused: "
                            f"{r.returncode} {r.stderr!r}")

        run("claim_simulator.py", PHONE, "--run", "run-a", tmp=tmp)
        r = run("claim_simulator.py", PHONE, "--run", "run-b", tmp=tmp)
        if r.returncode != 3 or "simulator" in r.stderr.lower() or "run-a" not in r.stderr:
            failures.append(f"a held phone is described as a simulator or without its holder: "
                            f"{r.returncode} {r.stderr!r}")
        r = run("claim_simulator.py", PHONE, "--run", "run-b", "--release", tmp=tmp)
        if r.returncode != 3 or "simulator" in r.stderr.lower():
            failures.append(f"phone release refusal is wrong or calls it a simulator: "
                            f"{r.returncode} {r.stderr!r}")

        r = run("claim_simulator.py", UDID, "--run", "run-b", "--steal", tmp=tmp)
        if r.returncode != 0 or json.loads(r.stdout)["heldBy"] != "run-b":
            failures.append(f"steal did not take the lock: {r.returncode} {r.stdout}")

        r = run("claim_simulator.py", UDID, "--run", "run-b", "--release", tmp=tmp)
        if r.returncode != 0:
            failures.append(f"holder could not release: {r.returncode} {r.stderr}")

        r = run("capture_slice.sh", "--simulator", UDID, "--run", "run-b",
                "--out", f"{tmp}/x.png", tmp=tmp)
        if r.returncode != 3 or "claim" not in r.stderr:
            failures.append(f"capture ran against an unclaimed simulator: {r.returncode} {r.stderr!r}")

        r = run("claim_simulator.py", UDID, "--run", "run-b", "--release", tmp=tmp)
        if r.returncode != 0:
            failures.append(f"releasing an unheld lock is not a no-op: {r.returncode}")

    for f in failures:
        print("FAIL:", f)
    print("PASS: one run holds a simulator or phone at a time, and capture needs the claim"
          if not failures else f"{len(failures)} failure(s)")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
