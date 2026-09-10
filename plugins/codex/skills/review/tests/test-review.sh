#!/usr/bin/env bash
# ABOUTME: Tests review.sh, the standalone local codex review of a commit range, against the stub
# ABOUTME: codex: arguments, sandbox and schema flags, prompt contents, output file, failure exit.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
REVIEW="$HERE/../review.sh"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/codex-review-test.XXXXXX")"
trap 'rm -rf "$SCRATCH"' EXIT
FAILS=0
assert() { local d="$1"; shift; if "$@" >/dev/null 2>&1; then echo "PASS: $d"; else echo "FAIL: $d  [cmd: $*]"; FAILS=$((FAILS+1)); fi; }
assert_eq() { if [ "$2" = "$3" ]; then echo "PASS: $1"; else echo "FAIL: $1  [expected '$2' got '$3']"; FAILS=$((FAILS+1)); fi; }

export STUB_DIR="$SCRATCH/stub"; mkdir -p "$STUB_DIR"
export IMPLEMENT_CODEX="$HERE/../../implement/tests/stub-codex/codex"
FIX="$SCRATCH/repo"
git init -q -b main "$FIX"
git -C "$FIX" config user.email test@test && git -C "$FIX" config user.name test
echo one > "$FIX/a.txt" && git -C "$FIX" add a.txt && git -C "$FIX" commit -qm base
BASE="$(git -C "$FIX" rev-parse HEAD)"
echo two > "$FIX/a.txt" && git -C "$FIX" commit -qam change
HEAD_SHA="$(git -C "$FIX" rev-parse HEAD)"

# ---------- usage ----------
set +e; "$REVIEW" > "$SCRATCH/usage.log" 2>&1; RC=$?; set -e 2>/dev/null || true
assert_eq "missing arguments exit 1" 1 "$RC"
assert "missing arguments print usage" grep -q '^Usage:' "$SCRATCH/usage.log"

# ---------- happy path ----------
set +e; "$REVIEW" "$FIX" "$BASE" "$HEAD_SHA" "$SCRATCH/review.json" specs/x/spec.md > "$SCRATCH/run.log" 2>&1; RC=$?; set -e 2>/dev/null || true
assert_eq "review exits 0" 0 "$RC"
assert_eq "codex invoked once" 1 "$(wc -l < "$STUB_DIR/calls-REVIEW" | tr -d ' ')"
assert "review runs at high reasoning effort" grep -q '^REVIEW cfg=model_reasoning_effort=high dir=' "$STUB_DIR/invocations.log"
assert "review runs read-only against the findings schema" grep -q '^REVIEW sandbox=read-only schema=review-schema.json dir=' "$STUB_DIR/invocations.log"
assert "prompt names both commits, the spec and must-fix" grep -q "^REVIEW prompt=.*$BASE.*$HEAD_SHA.*specs/x/spec.md.*must-fix" "$STUB_DIR/invocations.log"
assert_eq "findings written as JSON" '{"findings":[]}' "$(cat "$SCRATCH/review.json" 2>/dev/null)"
assert "result line names the file" grep -q "^codex:review: result: $SCRATCH/review.json$" "$SCRATCH/run.log"

# ---------- codex failure ----------
set +e; STUB_REVIEW_FAIL=1 "$REVIEW" "$FIX" "$BASE" "$HEAD_SHA" "$SCRATCH/review2.json" > "$SCRATCH/fail.log" 2>&1; RC=$?; set -e 2>/dev/null || true
assert_eq "codex failure exits 1" 1 "$RC"
assert "codex failure names the log" grep -q '^codex:review: codex review failed (see .*review2.log)$' "$SCRATCH/fail.log"

if [ "$FAILS" -gt 0 ]; then echo "$FAILS TEST(S) FAILED"; exit 1; fi
echo "ALL TESTS PASSED"
