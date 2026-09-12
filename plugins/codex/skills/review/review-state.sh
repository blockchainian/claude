#!/usr/bin/env bash
# ABOUTME: Polls GitHub for the cloud reviewer bot's PullRequestReview on one PR head until it
# ABOUTME: posts or reacts clean, classifies its findings, and prints one verdict JSON.
set -u

GH=${GH:-gh}
REVIEW_STATE_TIMEOUT_S=${REVIEW_STATE_TIMEOUT_S:-900}
REVIEW_STATE_INTERVAL_S=${REVIEW_STATE_INTERVAL_S:-15}
REVIEW_STATE_BOT_LOGIN=${REVIEW_STATE_BOT_LOGIN:-chatgpt-codex-connector}

usage="usage: review-state.sh <pr-number> <head-sha> [out-file]"
PR=${1:-}
HEAD=${2:-}
[ -n "$PR" ] && [ -n "$HEAD" ] || {
  echo "$usage" >&2
  exit 2
}
OUT_FILE=${3:-$(mktemp -t review-state.XXXXXX.json)}
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CLASSIFY="$SCRIPT_DIR/classify-severity.sh"

log() { printf '%s\n' "$*" >&2; }

# The cloud bot's login is "chatgpt-codex-connector" via GraphQL author.login on a review and
# "chatgpt-codex-connector[bot]" via reactions.user.login — a substring match covers both.
QUERY='query($owner: String!, $name: String!, $pr: Int!) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $pr) {
      reviews(last: 50) { nodes { submittedAt author { login } commit { oid } } }
      reactions(last: 50) { nodes { createdAt content user { login } } }
      reviewThreads(first: 100) { nodes { isResolved path line comments(first: 10) { nodes { body } } } }
    }
  }
}'

repo_owner_name() {
  local raw
  raw=$("$GH" api repos/{owner}/{repo} 2>/dev/null) || return 1
  python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["owner"]["login"]); print(d["name"])' <<<"$raw"
}

head_commit_time() { # <owner> <name> <sha>
  local raw
  raw=$("$GH" api "repos/$1/$2/commits/$3" 2>/dev/null) || return 1
  python3 -c 'import json,sys; print(json.load(sys.stdin)["commit"]["committer"]["date"])' <<<"$raw"
}

poll_once() { "$GH" api graphql -F owner="$OWNER" -F name="$NAME" -F pr="$PR" -f query="$QUERY" 2>/dev/null; }

# state_of <graphql-json> — prints "reviewed", "clean" or "pending". A clean re-review posts no
# PullRequestReview object at all, only a thumbs-up reaction (see codex-review-bot memory); a
# review is matched by commit.oid == head, a reaction by being newer than the head's commit time.
state_of() {
  python3 - "$1" "$HEAD" "$HEAD_TIME" "$REVIEW_STATE_BOT_LOGIN" <<'PY'
import json, sys
raw, head, head_time, bot = sys.argv[1:5]
d = json.loads(raw)
pr = d["data"]["repository"]["pullRequest"]
reviewed = any(
    (r.get("commit") or {}).get("oid") == head and bot in (r.get("author") or {}).get("login", "")
    for r in pr["reviews"]["nodes"] if r.get("submittedAt")
)
clean = any(
    r.get("content") == "THUMBS_UP" and r.get("createdAt", "") > head_time
    and bot in (r.get("user") or {}).get("login", "")
    for r in pr["reactions"]["nodes"]
)
print("reviewed" if reviewed else "clean" if clean else "pending")
PY
}

threads_json() {
  python3 - "$1" <<'PY'
import json, sys
d = json.loads(sys.argv[1])
nodes = d["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"]
out = [
    {
        "path": n["path"],
        "line": n.get("line"),
        "isResolved": n["isResolved"],
        "body": "\n".join(c["body"] for c in n["comments"]["nodes"]),
    }
    for n in nodes
]
print(json.dumps(out))
PY
}

emit() { # emit <state> <must-fix-json> <nits-json>
  python3 - "$PR" "$HEAD" "$1" "$2" "$3" <<'PY'
import json, sys
pr, head, state, must_fix, nits = sys.argv[1:6]
print(json.dumps({"pr": int(pr), "head": head, "state": state,
                   "must_fix": json.loads(must_fix), "nits": json.loads(nits)}))
PY
}

command -v "$GH" >/dev/null || { log "gh binary '$GH' not found"; exit 1; }

REPO_INFO=$(repo_owner_name) || { log "cannot resolve the repository for PR $PR"; exit 1; }
OWNER=$(sed -n '1p' <<<"$REPO_INFO")
NAME=$(sed -n '2p' <<<"$REPO_INFO")
[ -n "$OWNER" ] && [ -n "$NAME" ] || { log "cannot resolve owner/repo for PR $PR"; exit 1; }

HEAD_TIME=$(head_commit_time "$OWNER" "$NAME" "$HEAD") || { log "cannot resolve commit $HEAD"; exit 1; }
[ -n "$HEAD_TIME" ] || { log "cannot resolve commit $HEAD"; exit 1; }

START=$(date +%s)
INTERVAL=$REVIEW_STATE_INTERVAL_S
STATE=timeout
RAW='{}'
while :; do
  ELAPSED=$(($(date +%s) - START))
  if [ "$ELAPSED" -ge "$REVIEW_STATE_TIMEOUT_S" ]; then
    STATE=timeout
    break
  fi

  if FETCHED=$(poll_once) && [ -n "$FETCHED" ]; then
    RAW=$FETCHED
    S=$(state_of "$RAW")
    if [ "$S" != pending ]; then
      STATE=$S
      break
    fi
  else
    log "poll failed for PR $PR"
  fi

  log "polling PR $PR head $HEAD: pending"
  REMAINING=$((REVIEW_STATE_TIMEOUT_S - ELAPSED))
  SLEEP=$INTERVAL
  [ "$SLEEP" -gt "$REMAINING" ] && SLEEP=$REMAINING
  sleep "$SLEEP"
  INTERVAL=$((INTERVAL * 2))
  [ "$INTERVAL" -gt 60 ] && INTERVAL=60
done

MUST_FIX='[]'
NITS='[]'
if [ "$STATE" = reviewed ]; then
  THREADS=$(threads_json "$RAW")
  CLASSIFIED=$(printf '%s' "$THREADS" | "$CLASSIFY")
  MUST_FIX=$(python3 -c 'import json,sys; print(json.dumps(json.loads(sys.argv[1])["must_fix"]))' "$CLASSIFIED")
  NITS=$(python3 -c 'import json,sys; print(json.dumps(json.loads(sys.argv[1])["nits"]))' "$CLASSIFIED")
fi

OUT_JSON=$(emit "$STATE" "$MUST_FIX" "$NITS")
printf '%s\n' "$OUT_JSON" | tee "$OUT_FILE"
[ "$STATE" != timeout ]
