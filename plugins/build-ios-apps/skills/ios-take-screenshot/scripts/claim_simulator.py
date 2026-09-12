#!/usr/bin/env python3
"""Hold one simulator or phone for one capture run, so two agents never drive it at once.

XcodeBuildMCP scrolls whatever its session default points at and gives no
per-call way to name a simulator, so a second agent capturing the same
simulator silently retargets the first one's swipes. On a phone, WebDriverAgent
serves one session, so a second Appium session ends the first one's mid-run.
The lock is a file per UDID under TMPDIR, which every session of the same user
shares. A claim on a UDID another run holds exits 3 and says who holds it and
for how long; the later agent decides whether to wait or abort. Pass --steal only for a run
you know is dead. --release drops a claim this run holds.

Prints JSON. Exit 0 claimed or released, 2 bad arguments, 3 held by another run.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
from pathlib import Path


def lock_path(udid: str) -> Path:
    return Path(os.environ.get("TMPDIR") or "/tmp") / f"ios-screenshot-lock.{udid}.json"


def read(path: Path) -> dict | None:
    try:
        return json.loads(path.read_text())
    except (FileNotFoundError, json.JSONDecodeError):
        return None


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("udid", help="simulator or phone UDID; never \"booted\"")
    ap.add_argument("--run", required=True, help="RUN_ID of the capture run")
    ap.add_argument("--release", action="store_true", help="drop this run's claim")
    ap.add_argument("--steal", action="store_true",
                    help="take the UDID from a run that is known to be dead")
    args = ap.parse_args()

    if args.udid == "booted":
        print("\"booted\" names no simulator; pass the UDID", file=sys.stderr)
        return 2

    path = lock_path(args.udid)
    held = read(path)
    other = held is not None and held.get("run") != args.run
    now = time.time()

    if args.release:
        if other:
            print(json.dumps({"released": False, "udid": args.udid,
                              "heldBy": held["run"]}))
            print(f"{args.udid} is held by run {held['run']}, not {args.run}",
                  file=sys.stderr)
            return 3
        path.unlink(missing_ok=True)
        print(json.dumps({"released": True, "udid": args.udid}))
        return 0

    if other and not args.steal:
        age = int(now - held.get("since", now))
        print(json.dumps({"claimed": False, "udid": args.udid, "heldBy": held["run"],
                          "since": held.get("since"), "ageSeconds": age,
                          "lock": str(path)}))
        print(f"{args.udid} is in use by run {held['run']} for {age}s. "
              f"Wait and claim again when it is released, or abort and pick another "
              f"target. Only if that run is known to be dead: --steal.",
              file=sys.stderr)
        return 3

    path.write_text(json.dumps({"run": args.run, "since": now, "pid": os.getpid()}))
    print(json.dumps({"claimed": True, "udid": args.udid, "heldBy": args.run,
                      "stolenFrom": held["run"] if other else None, "lock": str(path)}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
