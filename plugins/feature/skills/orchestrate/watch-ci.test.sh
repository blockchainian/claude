#!/bin/bash
# ABOUTME: Tests for watch-ci.sh — stubs GH with a fake `gh pr checks` that goes pending then
# ABOUTME: terminal, asserting the consolidated verdict JSON shape and exit code. No live network.
set -u
cd "$(dirname "$0")" || exit 1
SCRIPTS_DIR=$(pwd)

PASS=0
FAIL=0

report() { # <name> <pass|fail> [detail]
  if [ "$2" = pass ]; then
    PASS=$((PASS + 1))
    printf 'ok   %s\n' "$1"
  else
    FAIL=$((FAIL + 1))
    printf 'FAIL %s\n     %s\n' "$1" "${3:-}"
  fi
}

# assert_json <name> <json> <python-expr over `d`>
assert_json() {
  local out
  out=$(python3 - "$2" "$3" <<'PY' 2>&1
import json, sys
try:
    d = json.loads(sys.argv[1])
except Exception as e:
    print("not JSON: %s | stdout=%r" % (e, sys.argv[1][:300]))
    sys.exit(1)
print("" if eval(sys.argv[2]) else "expression false: %s | %s" % (sys.argv[2], json.dumps(d)[:300]))
PY
  )
  if [ -n "$out" ]; then report "$1" fail "$out"; else report "$1" pass; fi
}

FIXTURE=$(mktemp -d)
trap 'rm -rf "$FIXTURE"' EXIT

# fake gh: `pr checks <ref> --json name,state,bucket` reports pending for the first two calls,
# then a terminal state (success or failure, per FAKE_GH_MODE). Every other invocation fails, so
# the fallback `gh run list`/`gh run view` path is exercised by a separate stub per test.
FAKE_GH="$FIXTURE/fake-gh.sh"
cat >"$FAKE_GH" <<'SH'
#!/bin/bash
set -u
if [ "$1" = pr ] && [ "$2" = checks ]; then
  count_file="$FAKE_GH_STATE_DIR/count"
  n=$(($(cat "$count_file" 2>/dev/null || echo 0) + 1))
  printf '%s' "$n" >"$count_file"
  if [ "$n" -lt 3 ]; then
    printf '[{"name":"CI","state":"PENDING","bucket":"pending"}]\n'
    exit 0
  fi
  if [ "$FAKE_GH_MODE" = fail ]; then
    printf '[{"name":"CI","state":"FAILURE","bucket":"fail"}]\n'
  else
    printf '[{"name":"CI","state":"SUCCESS","bucket":"pass"}]\n'
  fi
  exit 0
fi
echo "fake-gh: unhandled args: $*" >&2
exit 1
SH
chmod +x "$FAKE_GH"

run_watch() { # <ref> [out-file] — prints stdout; stderr to $ERR_FILE; sets RC
  ERR_FILE=$(mktemp)
  OUT=$(GH="$FAKE_GH" FAKE_GH_STATE_DIR="$STATE_DIR" FAKE_GH_MODE="$MODE" \
    WATCH_TIMEOUT_S=10 WATCH_INTERVAL_S=0 \
    "$SCRIPTS_DIR/watch-ci.sh" "$@" 2>"$ERR_FILE")
  RC=$?
}

# ---------- pending -> success ----------

STATE_DIR=$(mktemp -d)
MODE=success
OUT_FILE="$FIXTURE/success.json"
run_watch pr-123 "$OUT_FILE"
[ "$RC" -eq 0 ] && report "pending-then-success exits 0" pass ||
  report "pending-then-success exits 0" fail "rc=$RC $(cat "$ERR_FILE")"
assert_json "pending-then-success verdict shape" "$OUT" \
  "d['ref'] == 'pr-123' and d['state'] == 'completed' and d['conclusion'] == 'success' and isinstance(d['checks'], list) and isinstance(d['elapsed_s'], int)"
assert_json "pending-then-success reports the one check as concluded" "$OUT" \
  "d['checks'] == [{'name': 'CI', 'conclusion': 'success', 'pending': False}]"
if [ -f "$OUT_FILE" ] && [ "$(cat "$OUT_FILE")" = "$OUT" ]; then
  report "verdict is written to the out-file too" pass
else
  report "verdict is written to the out-file too" fail "out-file=$(cat "$OUT_FILE" 2>&1)"
fi
if [ "$(printf '%s' "$OUT" | wc -l | tr -d ' ')" -le 1 ]; then
  report "stdout carries nothing but the verdict JSON" pass
else
  report "stdout carries nothing but the verdict JSON" fail "stdout had extra lines: $OUT"
fi

# ---------- pending -> failure ----------

STATE_DIR=$(mktemp -d)
MODE=fail
run_watch pr-456
[ "$RC" -ne 0 ] && report "pending-then-failure exits non-zero" pass ||
  report "pending-then-failure exits non-zero" fail "rc=$RC"
assert_json "pending-then-failure verdict shape" "$OUT" \
  "d['ref'] == 'pr-456' and d['state'] == 'completed' and d['conclusion'] == 'failure'"

# ---------- missing ref argument ----------

ERR_FILE=$(mktemp)
GH="$FAKE_GH" "$SCRIPTS_DIR/watch-ci.sh" >/dev/null 2>"$ERR_FILE"
RC=$?
[ "$RC" -eq 2 ] && report "missing ref argument exits 2" pass ||
  report "missing ref argument exits 2" fail "rc=$RC"

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
