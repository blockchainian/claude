#!/usr/bin/env bash
# ABOUTME: Tests classify-severity.sh against sample cloud-reviewer comment bodies (the native
# ABOUTME: P0/P1/P2 badge convention), asserting the must-fix/nit split and resolved-thread drop.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
CLASSIFY="$HERE/../classify-severity.sh"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/classify-severity-test.XXXXXX")"
trap 'rm -rf "$SCRATCH"' EXIT
FAILS=0
assert() { local d="$1"; shift; if "$@" >/dev/null 2>&1; then echo "PASS: $d"; else echo "FAIL: $d  [cmd: $*]"; FAILS=$((FAILS+1)); fi; }
assert_eq() { if [ "$2" = "$3" ]; then echo "PASS: $1"; else echo "FAIL: $1  [expected '$2' got '$3']"; FAILS=$((FAILS+1)); fi; }

# Real bodies, from PR 623 (0xbabedead/chadwallet), the cloud reviewer's native format.
cat > "$SCRATCH/threads.json" <<'EOF'
[
  {
    "path": "proxy/src/birdEyeWebSocketHub.ts",
    "line": 2210,
    "isResolved": false,
    "body": "**<sub><sub>![P1 Badge](https://img.shields.io/badge/P1-orange?style=flat)</sub></sub>  Limit holder subscriptions to token-detail consumers**\n\nThis unconditionally opens a GMGN holder subscription for every SUBSCRIBE_TOKEN_STATS address.\n\nUseful? React with 👍 / 👎."
  },
  {
    "path": "scraper/src/gmgnActivityPool.ts",
    "line": 118,
    "isResolved": false,
    "body": "**<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Allocate freed IP slots fairly across pools**\n\nActivity pool redistribution can starve the holder pool of freed capacity.\n\nUseful? React with 👍 / 👎."
  },
  {
    "path": "proxy/src/birdEyeWebSocketHub.ts",
    "line": 1980,
    "isResolved": true,
    "body": "**<sub><sub>![P1 Badge](https://img.shields.io/badge/P1-orange?style=flat)</sub></sub>  Preserve per-address detail intent for holder subscriptions**\n\nAlready fixed and resolved; must not resurface.\n\nUseful? React with 👍 / 👎."
  },
  {
    "path": "mobile/features/token/hooks/useLiveTokenData.ts",
    "line": 44,
    "isResolved": false,
    "body": "A plain comment carrying no severity badge at all."
  }
]
EOF

OUT=$(cat "$SCRATCH/threads.json" | "$CLASSIFY")
RC=$?
assert_eq "exits 0" 0 "$RC"

check() { python3 -c '
import json, sys
d = json.loads(sys.argv[2])
sys.exit(0 if eval(sys.argv[1]) else 1)
' "$1" "$OUT"; }

assert "P1 unresolved thread is must-fix" check "len(d['must_fix']) == 1"
assert "must-fix entry names the file" check "d['must_fix'][0]['file'] == 'proxy/src/birdEyeWebSocketHub.ts'"
assert "must-fix entry keeps the line" check "d['must_fix'][0]['line'] == 2210"
assert "must-fix claim is the badge title, not the raw body" check "d['must_fix'][0]['claim'] == 'Limit holder subscriptions to token-detail consumers'"
assert "P2 unresolved thread is a nit" check "len(d['nits']) == 2"
assert "unbadged unresolved thread defaults to nit" check "any(n['file'].endswith('useLiveTokenData.ts') for n in d['nits'])"
assert "resolved P1 thread is dropped entirely" check "not any('Preserve per-address' in (f.get('claim') or '') for f in d['must_fix'] + d['nits'])"

# ---------- reading from a file argument instead of stdin ----------
OUT2=$("$CLASSIFY" "$SCRATCH/threads.json")
assert_eq "file-argument form matches stdin form" "$OUT" "$OUT2"

# ---------- empty input ----------
OUT3=$(printf '[]' | "$CLASSIFY")
assert_eq "empty threads yield empty verdict" '{"must_fix": [], "nits": []}' "$OUT3"

if [ "$FAILS" -gt 0 ]; then echo "$FAILS TEST(S) FAILED"; exit 1; fi
echo "ALL TESTS PASSED"
