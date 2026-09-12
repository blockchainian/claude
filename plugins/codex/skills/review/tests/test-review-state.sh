#!/usr/bin/env bash
# ABOUTME: Tests review-state.sh, the poller that waits for the cloud reviewer bot's PullRequestReview
# ABOUTME: on one PR head, against a stub gh: pending->posted-with-must-fix and pending->clean-reaction.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
REVIEW_STATE="$HERE/../review-state.sh"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/review-state-test.XXXXXX")"
trap 'rm -rf "$SCRATCH"' EXIT
FAILS=0
assert() { local d="$1"; shift; if "$@" >/dev/null 2>&1; then echo "PASS: $d"; else echo "FAIL: $d  [cmd: $*]"; FAILS=$((FAILS+1)); fi; }
assert_eq() { if [ "$2" = "$3" ]; then echo "PASS: $1"; else echo "FAIL: $1  [expected '$2' got '$3']"; FAILS=$((FAILS+1)); fi; }
check() { python3 -c '
import json, sys
d = json.loads(sys.argv[2])
sys.exit(0 if eval(sys.argv[1]) else 1)
' "$1" "$OUT"; }

HEAD_A=aaaaaaa1111111111111111111111111111111a
HEAD_B=bbbbbbb2222222222222222222222222222222b
HEAD_TIME=2026-09-11T15:00:00Z

# fake gh serves three request shapes: `api repos/{owner}/{repo}` (repo identity), `api
# repos/OWNER/NAME/commits/SHA` (head commit time), and `api graphql ...` (reviews/reactions/
# threads). GraphQL replies pending for the first two polls, then a terminal state driven by
# FAKE_GH_MODE: "must-fix" posts a submitted review on the head with one P1 and one P2 open
# thread; "clean" posts only a thumbs-up reaction newer than the head, no review.
make_fake_gh() {
  local dir="$1"
  cat > "$dir/fake-gh.sh" <<SH
#!/bin/bash
set -u
count_file="\$FAKE_GH_STATE_DIR/count"
if [ "\$1" = api ] && [ "\$2" = "repos/{owner}/{repo}" ]; then
  printf '%s\n' '{"owner":{"login":"acme"},"name":"widget"}'
  exit 0
fi
if [ "\$1" = api ] && [ "\$2" = "repos/acme/widget/commits/$HEAD_A" ]; then
  printf '%s\n' '{"commit":{"committer":{"date":"$HEAD_TIME"}}}'
  exit 0
fi
if [ "\$1" = api ] && [ "\$2" = graphql ]; then
  n=\$((\$(cat "\$count_file" 2>/dev/null || echo 0) + 1))
  printf '%s' "\$n" > "\$count_file"
  if [ "\$n" -lt 3 ]; then
    printf '%s\n' '{"data":{"repository":{"pullRequest":{"reviews":{"nodes":[]},"reactions":{"nodes":[]},"reviewThreads":{"nodes":[]}}}}}'
    exit 0
  fi
  # Passed to printf's %s, not its format string — the real cloud bot's comment bodies carry
  # literal \n paragraph breaks, and a format string (rather than an argument) would have printf
  # expand those into raw newlines, breaking the JSON's string syntax.
  if [ "\$FAKE_GH_MODE" = clean ]; then
    printf '%s\n' '{"data":{"repository":{"pullRequest":{
      "reviews":{"nodes":[]},
      "reactions":{"nodes":[{"createdAt":"2026-09-11T15:05:00Z","content":"THUMBS_UP","user":{"login":"chatgpt-codex-connector[bot]"}}]},
      "reviewThreads":{"nodes":[]}
    }}}}'
  else
    printf '%s\n' '{"data":{"repository":{"pullRequest":{
      "reviews":{"nodes":[{"submittedAt":"2026-09-11T15:05:00Z","author":{"login":"chatgpt-codex-connector"},"commit":{"oid":"$HEAD_A"}}]},
      "reactions":{"nodes":[]},
      "reviewThreads":{"nodes":[
        {"path":"proxy/src/a.ts","line":10,"isResolved":false,"comments":{"nodes":[{"author":{"login":"chatgpt-codex-connector"},"body":"**<sub><sub>![P1 Badge](https://img.shields.io/badge/P1-orange?style=flat)</sub></sub>  Fix the thing**\n\nDetail.\n\nUseful? React with 👍 / 👎."}]}},
        {"path":"proxy/src/b.ts","line":20,"isResolved":false,"comments":{"nodes":[{"author":{"login":"chatgpt-codex-connector"},"body":"**<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Nit the thing**\n\nDetail.\n\nUseful? React with 👍 / 👎."}]}}
      ]}
    }}}}'
  fi
  exit 0
fi
echo "fake-gh: unhandled args: \$*" >&2
exit 1
SH
  chmod +x "$dir/fake-gh.sh"
}

run_review_state() { # <head> [out-file]
  ERR_FILE=$(mktemp)
  OUT=$(GH="$FAKE_GH" FAKE_GH_STATE_DIR="$STATE_DIR" FAKE_GH_MODE="$MODE" \
    REVIEW_STATE_TIMEOUT_S=10 REVIEW_STATE_INTERVAL_S=0 \
    "$REVIEW_STATE" 42 "$@" 2>"$ERR_FILE")
  RC=$?
}

# ---------- pending -> posted with must-fix ----------
FAKE_DIR="$SCRATCH/fake1"; mkdir -p "$FAKE_DIR"
make_fake_gh "$FAKE_DIR"
FAKE_GH="$FAKE_DIR/fake-gh.sh"
STATE_DIR=$(mktemp -d)
MODE=must-fix
OUT_FILE="$SCRATCH/reviewed.json"
run_review_state "$HEAD_A" "$OUT_FILE"
assert_eq "reviewed run exits 0" 0 "$RC"
assert "reviewed verdict shape" check "d['pr'] == 42 and d['head'] == '$HEAD_A' and d['state'] == 'reviewed'"
assert "reviewed verdict carries the P1 as must-fix" check "len(d['must_fix']) == 1 and d['must_fix'][0]['file'] == 'proxy/src/a.ts' and d['must_fix'][0]['claim'] == 'Fix the thing'"
assert "reviewed verdict carries the P2 as a nit" check "len(d['nits']) == 1 and d['nits'][0]['file'] == 'proxy/src/b.ts'"
if [ -f "$OUT_FILE" ] && [ "$(cat "$OUT_FILE")" = "$OUT" ]; then
  echo "PASS: verdict is written to the out-file too"
else
  echo "FAIL: verdict is written to the out-file too  [out-file=$(cat "$OUT_FILE" 2>&1)]"
  FAILS=$((FAILS+1))
fi

# ---------- pending -> clean reaction ----------
FAKE_DIR2="$SCRATCH/fake2"; mkdir -p "$FAKE_DIR2"
make_fake_gh "$FAKE_DIR2"
FAKE_GH="$FAKE_DIR2/fake-gh.sh"
STATE_DIR=$(mktemp -d)
MODE=clean
run_review_state "$HEAD_A"
assert_eq "clean run exits 0" 0 "$RC"
assert "clean verdict shape" check "d['state'] == 'clean' and d['must_fix'] == [] and d['nits'] == []"

# ---------- timeout ----------
FAKE_DIR3="$SCRATCH/fake3"; mkdir -p "$FAKE_DIR3"
cat > "$FAKE_DIR3/fake-gh.sh" <<'SH'
#!/bin/bash
set -u
if [ "$1" = api ] && [ "$2" = "repos/{owner}/{repo}" ]; then
  printf '{"owner":{"login":"acme"},"name":"widget"}\n'
  exit 0
fi
if [ "$1" = api ] && [ "$2" = graphql ]; then
  printf '{"data":{"repository":{"pullRequest":{"reviews":{"nodes":[]},"reactions":{"nodes":[]},"reviewThreads":{"nodes":[]}}}}}\n'
  exit 0
fi
printf '{"commit":{"committer":{"date":"2026-09-11T15:00:00Z"}}}\n'
SH
chmod +x "$FAKE_DIR3/fake-gh.sh"
FAKE_GH="$FAKE_DIR3/fake-gh.sh"
STATE_DIR=$(mktemp -d)
MODE=must-fix
ERR_FILE=$(mktemp)
OUT=$(GH="$FAKE_GH" FAKE_GH_STATE_DIR="$STATE_DIR" REVIEW_STATE_TIMEOUT_S=1 REVIEW_STATE_INTERVAL_S=1 \
  "$REVIEW_STATE" 42 "$HEAD_B" 2>"$ERR_FILE")
RC=$?
assert_eq "timeout exits non-zero" 1 "$RC"
assert "timeout verdict shape" check "d['state'] == 'timeout' and d['must_fix'] == [] and d['nits'] == []"

# ---------- missing arguments ----------
ERR_FILE=$(mktemp)
GH="$FAKE_GH" "$REVIEW_STATE" >/dev/null 2>"$ERR_FILE"
RC=$?
assert_eq "missing arguments exit 2" 2 "$RC"

if [ "$FAILS" -gt 0 ]; then echo "$FAILS TEST(S) FAILED"; exit 1; fi
echo "ALL TESTS PASSED"
