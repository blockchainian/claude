#!/usr/bin/env bash
# ABOUTME: Tests resolve-threads.sh --dry-run: the operation plan built from dispositions (reaction,
# ABOUTME: reply, resolve; skipped local findings; empty input), without any live GitHub call.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
RESOLVE="$HERE/../resolve-threads.sh"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/resolve-threads-test.XXXXXX")"
trap 'rm -rf "$SCRATCH"' EXIT
FAILS=0
assert() { local d="$1"; shift; if "$@" >/dev/null 2>&1; then echo "PASS: $d"; else echo "FAIL: $d  [cmd: $*]"; FAILS=$((FAILS+1)); fi; }
assert_eq() { if [ "$2" = "$3" ]; then echo "PASS: $1"; else echo "FAIL: $1  [expected '$2' got '$3']"; FAILS=$((FAILS+1)); fi; }

cat > "$SCRATCH/dispositions.json" <<'EOF'
[
  { "thread_id": "PRRT_fixed", "comment_id": "PRRC_fixed", "disposition": "fixed" },
  { "thread_id": "PRRT_rej", "comment_id": "PRRC_rej", "disposition": "rejected", "reason": "reverts intended drop-in parity" },
  { "thread_id": "PRRT_norsn", "comment_id": "PRRC_norsn", "disposition": "rejected", "reason": "" },
  { "thread_id": "PRRT_nocmt", "comment_id": null, "disposition": "fixed" },
  { "thread_id": null, "comment_id": null, "disposition": "fixed", "reason": "local review, no thread" }
]
EOF

PLAN=$(cat "$SCRATCH/dispositions.json" | "$RESOLVE" --dry-run)
RC=$?
assert_eq "dry-run exits 0" 0 "$RC"

check() { python3 -c '
import json, sys
d = json.loads(sys.argv[2])
sys.exit(0 if eval(sys.argv[1]) else 1)
' "$1" "$PLAN"; }

assert "fixed: thumbs-up then resolve, in order" check \
  "[o for o in d if o.get('thread_id')=='PRRT_fixed' or o.get('comment_id')=='PRRC_fixed'] == [{'op':'react','comment_id':'PRRC_fixed','content':'THUMBS_UP'},{'op':'resolve','thread_id':'PRRT_fixed'}]"
assert "rejected with reason: thumbs-down, reply, resolve" check \
  "[o for o in d if o.get('thread_id')=='PRRT_rej' or o.get('comment_id')=='PRRC_rej'] == [{'op':'react','comment_id':'PRRC_rej','content':'THUMBS_DOWN'},{'op':'reply','thread_id':'PRRT_rej','body':'reverts intended drop-in parity'},{'op':'resolve','thread_id':'PRRT_rej'}]"
assert "rejected without reason: no reply op" check \
  "not any(o['op']=='reply' and o.get('thread_id')=='PRRT_norsn' for o in d)"
assert "rejected without reason: still reacts and resolves" check \
  "[o for o in d if o.get('thread_id')=='PRRT_norsn' or o.get('comment_id')=='PRRC_norsn'] == [{'op':'react','comment_id':'PRRC_norsn','content':'THUMBS_DOWN'},{'op':'resolve','thread_id':'PRRT_norsn'}]"
assert "null comment_id: resolve only, no react" check \
  "[o for o in d if o.get('thread_id')=='PRRT_nocmt'] == [{'op':'resolve','thread_id':'PRRT_nocmt'}]"
assert "null thread_id (local finding) contributes no op" check \
  "not any('local' in (o.get('body') or '') for o in d) and len(d) == 8"

# ---------- empty input ----------
OUT_EMPTY=$(printf '[]' | "$RESOLVE" --dry-run)
assert_eq "empty dispositions yield empty plan" '[]' "$OUT_EMPTY"

if [ "$FAILS" -gt 0 ]; then echo "$FAILS TEST(S) FAILED"; exit 1; fi
echo "ALL TESTS PASSED"
