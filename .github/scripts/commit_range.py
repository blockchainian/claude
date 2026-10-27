"""Resolve the validated commit range for CI privacy checks."""

import os
import re
import subprocess
import sys


def git(*args):
    return subprocess.check_output(["git", *args], stderr=subprocess.DEVNULL).decode(
        "utf-8", errors="replace"
    )


def commit_range():
    base = os.environ["PRIVACY_BASE"]
    head = os.environ["PRIVACY_HEAD"]
    event = os.environ["PRIVACY_EVENT"]
    if event not in ("push", "pull_request"):
        raise ValueError("Unsupported event")
    if not all(re.fullmatch(r"[0-9a-f]{40}", sha) for sha in (base, head)):
        raise ValueError("Invalid revision")
    git("rev-parse", "--verify", head + "^{commit}")
    if base == "0" * 40:
        if event != "push":
            raise ValueError("Missing pull request base")
        return head
    if event == "pull_request":
        base = git("merge-base", base, head).strip()
    else:
        git("rev-parse", "--verify", base + "^{commit}")
    return base + ".." + head


if __name__ == "__main__":
    try:
        print(commit_range())
    except (OSError, ValueError, KeyError, subprocess.SubprocessError):
        print("Commit range could not be resolved.", file=sys.stderr)
        sys.exit(2)
