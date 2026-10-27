"""Scan added lines for private terms without logging their contents."""

import ast
import json
import os
from pathlib import Path
import re
import subprocess
import sys

from commit_range import commit_range, git


def location(filename, line):
    # Escape control characters so filenames cannot create extra log lines.
    return json.dumps(filename, ensure_ascii=True)[1:-1] + ":" + str(line)


def denylist():
    content = os.environ.get("PRIVACY_DENYLIST", "")
    if not content:
        path = Path(git("rev-parse", "--show-toplevel").strip()) / ".privacyignore"
        if path.exists():
            content = path.read_text(encoding="utf-8")
    return [word.strip().casefold() for word in content.splitlines() if word.strip()]


def scan_denylist(staged=False):
    words = denylist()
    if not words:
        print("PRIVACY_DENYLIST is empty; skipping denylist scan.")
        return 0
    if staged:
        patches = [git("-c", "core.quotePath=true", "diff", "--cached",
                       "--unified=0", "--no-ext-diff", "--no-textconv",
                       "--no-renames", "--text")]
    else:
        revisions = commit_range()
        patches = (git("-c", "core.quotePath=true", "show", "--format=",
                       "--first-parent", "--root", "--unified=0", "--no-ext-diff",
                       "--no-textconv", "--no-renames", "--text", commit)
                   for commit in git("rev-list", "--reverse", revisions).splitlines())
    findings = set()
    for patch in patches:
        filename = None
        line_number = None
        for line in patch.split("\n"):
            if line.startswith("diff --git "):
                filename = None
                line_number = None
            elif line_number is None and line.startswith("+++ "):
                name = line[4:]
                if name.startswith('"'):
                    name = ast.literal_eval("b" + name).decode("utf-8", errors="replace")
                filename = name[2:] if name.startswith("b/") else None
            elif line.startswith("@@ "):
                match = re.match(r"@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@", line)
                if not match:
                    raise ValueError("Invalid patch")
                # A deleted file's hunks add no lines.
                line_number = int(match[1]) if filename is not None else None
            elif line_number is not None and line.startswith("+"):
                if any(word in line[1:].casefold() for word in words):
                    findings.add(location(filename, line_number))
                line_number += 1
            elif line_number is not None and line.startswith(" "):
                line_number += 1
    for entry in sorted(findings):
        print(entry)
    return int(bool(findings))


def main():
    try:
        # The pre-commit hook scans what is about to be committed.
        if sys.argv[1:] == ["staged"]:
            return scan_denylist(staged=True)
        if sys.argv[1:] == ["denylist"]:
            return scan_denylist()
        raise ValueError("Invalid scanner")
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError):
        # Tool errors can contain source text or local paths; never echo them.
        print("Privacy scan could not complete.", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
