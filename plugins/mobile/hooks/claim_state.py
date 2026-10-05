# ABOUTME: Serializes device-claim changes across processes with an OS-released file lock.
# ABOUTME: Atomic JSON writes keep capture readers from observing partially written claims.
from __future__ import annotations

from contextlib import contextmanager
import fcntl
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


@contextmanager
def locked(path: Path):
    # Keep the mutex file: unlinking it lets two processes lock different inodes.
    # The OS releases flock even if the process crashes; no stale-lock timeout.
    with Path(str(path) + ".mutex").open("a") as mutex:
        fcntl.flock(mutex, fcntl.LOCK_EX)
        yield mutex.fileno()


def read(path: Path) -> dict | None:
    try:
        held = json.loads(path.read_text())
    except FileNotFoundError:
        return None
    if not isinstance(held, dict) or not isinstance(held.get("run"), str) or not held["run"]:
        raise ValueError(f"Invalid device claim: {path}")
    return held


def write(path: Path, held: dict) -> None:
    fd, name = tempfile.mkstemp(prefix=path.name + ".", dir=path.parent)
    try:
        with os.fdopen(fd, "w") as out:
            json.dump(held, out)
        os.replace(name, path)
    finally:
        Path(name).unlink(missing_ok=True)


if __name__ == "__main__":
    with locked(Path(sys.argv[1])) as fd:
        # The child retains the same lock if this wrapper is interrupted.
        raise SystemExit(subprocess.run(sys.argv[2:], check=False, pass_fds=(fd,)).returncode)
