"""Scan added commits without writing their contents to the job log."""

import ast
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile


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


def location(filename, line):
    # Escape control characters so filenames cannot create extra log lines.
    return json.dumps(filename, ensure_ascii=True)[1:-1] + ":" + str(line)


def scan_secrets(*scope):
    config = Path(__file__).resolve().parents[2] / ".gitleaks.toml"
    with tempfile.TemporaryDirectory() as directory:
        report = Path(directory) / "report.json"
        result = subprocess.run(
            ["gitleaks", "git", "--config", str(config), *scope,
             "--redact", "--no-banner", "--report-format=json",
             "--report-path", str(report)],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        if result.returncode not in (0, 1):
            raise RuntimeError("Secret scanner failed")
        if result.returncode == 0:
            return 0
        findings = json.loads(report.read_text())
        if not findings:
            raise ValueError("Missing findings")
        for entry in sorted({location(item["File"], item["StartLine"]) for item in findings}):
            print(entry)
        return 1


def scan_denylist(revisions):
    words = [word.strip().casefold()
             for word in os.environ.get("PRIVACY_DENYLIST", "").splitlines()
             if word.strip()]
    if not words:
        print("PRIVACY_DENYLIST is empty; skipping denylist scan.")
        return 0
    findings = set()
    for commit in git("rev-list", "--reverse", revisions).splitlines():
        patch = git("-c", "core.quotePath=true", "show", "--format=",
                    "--first-parent", "--root", "--unified=0", "--no-ext-diff",
                    "--no-textconv", "--no-renames", "--text", commit)
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
            return scan_secrets("--pre-commit", "--staged")
        revisions = commit_range()
        if sys.argv[1:] == ["gitleaks"]:
            return scan_secrets("--log-opts=--diff-merges=first-parent " + revisions)
        if sys.argv[1:] == ["denylist"]:
            return scan_denylist(revisions)
        raise ValueError("Invalid scanner")
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError):
        # Tool errors can contain source text or local paths; never echo them.
        print("Privacy scan could not complete.", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
