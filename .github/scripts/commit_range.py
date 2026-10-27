"""Resolve the validated commit range for a GitHub Actions event."""

import json
import os
import re
import subprocess
import sys


def git(*args):
    return subprocess.check_output(["git", *args], stderr=subprocess.DEVNULL).decode(
        "utf-8", errors="replace"
    )


def commit_range():
    event = os.environ["GITHUB_EVENT_NAME"]
    if event not in ("push", "pull_request"):
        raise ValueError("Unsupported event")
    with open(os.environ["GITHUB_EVENT_PATH"], encoding="utf-8") as source:
        payload = json.load(source)
    if event == "pull_request":
        base = payload["pull_request"]["base"]["sha"]
        head = payload["pull_request"]["head"]["sha"]
    else:
        base = payload["before"]
        head = payload["after"]
    if not all(isinstance(sha, str) and re.fullmatch(r"[0-9a-fA-F]{40}", sha)
               for sha in (base, head)):
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
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
        print("Commit range could not be resolved.", file=sys.stderr)
        sys.exit(2)
