#!/usr/bin/env bash
# ABOUTME: Classifies the cloud reviewer's open PR-thread comments into must-fix vs nit using its
# ABOUTME: native P0/P1/P2 severity badge, so a caller can triage a cloud review the way it triages
# ABOUTME: a local one. Takes no network calls itself — review-state.sh feeds it live threads.
set -u

usage() {
  cat >&2 <<'EOF'
Usage: classify-severity.sh [threads.json]

Reads a JSON array of {path, line, isResolved, body} (default: stdin) — one entry per PR review
thread, body the concatenated comment text — and prints {must_fix: [...], nits: [...]} where each
entry is {file, line, severity, claim}.

Severity: a "![P0 Badge]" or "![P1 Badge]" marks must-fix; P2 and higher, or no badge at all, is
a nit. A resolved thread is dropped.
EOF
  exit 2
}

case "${1:-}" in -h|--help) usage ;; esac

# `python3 - <<'PY'` sources the script itself from stdin, so a script that also wants stdin
# data cannot `open("/dev/stdin")` inside the heredoc — that fd is already spent on the script
# text. Read stdin into a temp file up front instead when no file argument is given.
CLEANUP=""
trap '[ -n "$CLEANUP" ] && rm -f "$CLEANUP"' EXIT
if [ -n "${1:-}" ]; then
  IN="$1"
  [ -r "$IN" ] || usage
else
  IN="$(mktemp -t classify-severity.XXXXXX)"
  CLEANUP="$IN"
  cat > "$IN"
fi

python3 - "$IN" <<'PY'
import json
import re
import sys

with open(sys.argv[1]) as f:
    threads = json.load(f)

BADGE = re.compile(r'!\[P(\d+) Badge\]')
TITLE = re.compile(r'</sub></sub>\s*(.+?)\*\*')


def classify(thread):
    body = thread.get("body") or ""
    badge = BADGE.search(body)
    severity = "must-fix" if badge and int(badge.group(1)) <= 1 else "nit"
    title = TITLE.search(body)
    if title:
        claim = title.group(1).strip()
    else:
        first_line = body.strip().splitlines()[0] if body.strip() else ""
        claim = first_line[:200]
    return {"file": thread.get("path", ""), "line": thread.get("line") or 0, "severity": severity, "claim": claim}

findings = [classify(t) for t in threads if not t.get("isResolved")]
must_fix = [f for f in findings if f["severity"] == "must-fix"]
nits = [f for f in findings if f["severity"] == "nit"]
print(json.dumps({"must_fix": must_fix, "nits": nits}))
PY
