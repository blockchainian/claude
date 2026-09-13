#!/bin/bash
# ABOUTME: Polls gh for a ref's CI checks until every check concludes or a timeout, so the caller
# ABOUTME: makes one background launch + one read instead of many foreground `gh pr checks` polls.
set -u

GH=${GH:-gh}
WATCH_TIMEOUT_S=${WATCH_TIMEOUT_S:-600}
WATCH_INTERVAL_S=${WATCH_INTERVAL_S:-15}

usage="usage: watch-ci.sh <pr-number|branch|commit-sha> [out-file]"
REF=${1:-}
[ -n "$REF" ] || {
  echo "$usage" >&2
  exit 2
}
OUT_FILE=${2:-$(mktemp -t watch-ci.XXXXXX.json)}

log() { printf '%s\n' "$*" >&2; }

# fetch_checks <ref> — prints a JSON array of {name, conclusion, pending} on stdout and returns
# non-zero only if the ref could not be resolved to a PR or a workflow run at all.
fetch_checks() {
  local ref=$1 raw
  if raw=$("$GH" pr checks "$ref" --json name,state,bucket 2>/dev/null); then
    python3 - "$raw" <<'PY'
import json, sys
checks = json.loads(sys.argv[1])
out = [{"name": c["name"], "conclusion": c["state"].lower(), "pending": c["bucket"] == "pending"} for c in checks]
print(json.dumps(out))
PY
    return 0
  fi

  # No PR for this ref (a bare branch or commit SHA): fall back to its latest workflow run.
  local run_id
  if printf '%s' "$ref" | grep -Eq '^[0-9a-f]{7,40}$'; then
    run_id=$("$GH" run list -c "$ref" --limit 1 --json databaseId -q '.[0].databaseId' 2>/dev/null)
  else
    run_id=$("$GH" run list -b "$ref" --limit 1 --json databaseId -q '.[0].databaseId' 2>/dev/null)
  fi
  [ -n "$run_id" ] && [ "$run_id" != null ] || return 1

  raw=$("$GH" run view "$run_id" --json jobs 2>/dev/null) || return 1
  python3 - "$raw" <<'PY'
import json, sys
jobs = json.loads(sys.argv[1])["jobs"]
out = [
    {"name": j["name"], "conclusion": j["conclusion"] or "pending", "pending": j["status"] != "completed"}
    for j in jobs
]
print(json.dumps(out))
PY
}

# verdict <state> <checks-json> <elapsed> — prints the consolidated verdict JSON and exits 0
# only for a completed success (memory gate-deploy-chains-on-exit-codes).
verdict() {
  python3 - "$REF" "$1" "$2" "$3" <<'PY'
import json, sys
ref, state, checks_json, elapsed = sys.argv[1:5]
checks = json.loads(checks_json)
bad = ("failure", "cancelled", "timed_out", "action_required", "startup_failure")
failed = [c["conclusion"] for c in checks if c["conclusion"] in bad]
if failed:
    conclusion = failed[0]
elif not checks or any(c["pending"] for c in checks):
    conclusion = "pending"
else:
    conclusion = "success"
result = {"ref": ref, "state": state, "conclusion": conclusion, "checks": checks, "elapsed_s": int(elapsed)}
print(json.dumps(result))
sys.exit(0 if state == "completed" and conclusion == "success" else 1)
PY
}

# checks_done <checks-json> — exits 0 once every check has concluded (and at least one exists).
checks_done() {
  python3 -c 'import json,sys; c=json.loads(sys.argv[1]); sys.exit(0 if c and not any(x["pending"] for x in c) else 1)' "$1"
}

# checks_progress <checks-json> — one-line "N/M concluded" summary for the progress log.
checks_progress() {
  python3 - "$1" <<'PY'
import json, sys
checks = json.loads(sys.argv[1])
if not checks:
    print("no checks yet")
else:
    done = sum(1 for c in checks if not c["pending"])
    print(f"{done}/{len(checks)} concluded")
PY
}

START=$(date +%s)
INTERVAL=$WATCH_INTERVAL_S
CHECKS='[]'
STATE=timeout

while :; do
  ELAPSED=$(($(date +%s) - START))
  if [ "$ELAPSED" -ge "$WATCH_TIMEOUT_S" ]; then
    STATE=timeout
    break
  fi

  if FETCHED=$(fetch_checks "$REF"); then
    CHECKS=$FETCHED
  else
    log "$REF has no PR or workflow run yet"
    CHECKS='[]'
  fi

  if checks_done "$CHECKS"; then
    STATE=completed
    break
  fi

  log "polling $REF: $(checks_progress "$CHECKS")"

  REMAINING=$((WATCH_TIMEOUT_S - ELAPSED))
  SLEEP=$INTERVAL
  [ "$SLEEP" -gt "$REMAINING" ] && SLEEP=$REMAINING
  sleep "$SLEEP"
  INTERVAL=$((INTERVAL * 2))
  [ "$INTERVAL" -gt 60 ] && INTERVAL=60
done

FINAL_ELAPSED=$(($(date +%s) - START))
OUT_JSON=$(verdict "$STATE" "$CHECKS" "$FINAL_ELAPSED")
RC=$?
printf '%s\n' "$OUT_JSON" | tee "$OUT_FILE"
exit "$RC"
