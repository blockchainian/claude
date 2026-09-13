#!/usr/bin/env bash
# ABOUTME: Closes the loop on cloud-review PR threads by disposition — a reaction, an optional reply,
# ABOUTME: and resolving the thread — so a reader of the PR sees which findings ship handled.
set -u

GH=${GH:-gh}

usage() {
  cat >&2 <<'EOF'
Usage: resolve-threads.sh [dispositions.json]
       resolve-threads.sh --dry-run [dispositions.json]

Reads a JSON array of {thread_id, comment_id, disposition, reason} (default: stdin) and, for each
entry with a thread_id, closes the loop on that cloud-review thread:
  - disposition "fixed"    -> 👍 on the comment, then resolve the thread.
  - disposition "rejected" -> 👎 on the comment, a one-line reply naming `reason`, then resolve.
Entries without a thread_id (local-review findings, which are never GitHub threads) are skipped.
An empty array is a no-op. Resolving and reacting are idempotent, so re-running is safe.

--dry-run prints the planned operations as JSON instead of calling GitHub, so the plan can be
tested without a live PR.
EOF
  exit 2
}

DRY_RUN=0
case "${1:-}" in
  -h|--help) usage ;;
  --dry-run) DRY_RUN=1; shift ;;
esac

CLEANUP=""
trap '[ -n "$CLEANUP" ] && rm -f "$CLEANUP"' EXIT
if [ -n "${1:-}" ]; then
  IN="$1"
  [ -r "$IN" ] || usage
else
  IN="$(mktemp -t resolve-threads.XXXXXX)"
  CLEANUP="$IN"
  cat > "$IN"
fi

# Build the ordered operation plan from the dispositions — pure, so --dry-run can print it and a
# test can assert on it. reaction before reply before resolve, so a resolved thread still shows the
# verdict. Entries without a thread_id are dropped here, not in the executor.
PLAN=$(python3 - "$IN" <<'PY'
import json, sys

with open(sys.argv[1]) as f:
    entries = json.load(f)

ops = []
for e in entries:
    thread_id = e.get("thread_id")
    if not thread_id:
        continue
    comment_id = e.get("comment_id")
    disposition = e.get("disposition")
    reason = (e.get("reason") or "").strip()
    if disposition == "fixed":
        if comment_id:
            ops.append({"op": "react", "comment_id": comment_id, "content": "THUMBS_UP"})
    elif disposition == "rejected":
        if comment_id:
            ops.append({"op": "react", "comment_id": comment_id, "content": "THUMBS_DOWN"})
        if reason:
            ops.append({"op": "reply", "thread_id": thread_id, "body": reason})
    ops.append({"op": "resolve", "thread_id": thread_id})

print(json.dumps(ops))
PY
)

if [ "$DRY_RUN" = 1 ]; then
  printf '%s\n' "$PLAN"
  exit 0
fi

log() { printf '%s\n' "$*" >&2; }
command -v "$GH" >/dev/null || { log "gh binary '$GH' not found"; exit 1; }

REACT_MUTATION='mutation($subjectId: ID!, $content: ReactionContent!) {
  addReaction(input: {subjectId: $subjectId, content: $content}) { reaction { content } }
}'
REPLY_MUTATION='mutation($threadId: ID!, $body: String!) {
  addPullRequestReviewThreadReply(input: {pullRequestReviewThreadId: $threadId, body: $body}) { comment { id } }
}'
RESOLVE_MUTATION='mutation($threadId: ID!) {
  resolveReviewThread(input: {threadId: $threadId}) { thread { isResolved } }
}'

# One gh call per op; the plan is small (one PR's must-fixes). A failed op logs and continues so one
# stale id does not strand the rest.
FAILS=0
COUNT=$(python3 -c 'import json,sys; print(len(json.load(sys.stdin)))' <<<"$PLAN")
i=0
while [ "$i" -lt "$COUNT" ]; do
  OP=$(python3 -c 'import json,sys; print(json.load(sys.stdin)[int(sys.argv[1])]["op"])' "$i" <<<"$PLAN")
  case "$OP" in
    react)
      SUBJ=$(python3 -c 'import json,sys; print(json.load(sys.stdin)[int(sys.argv[1])]["comment_id"])' "$i" <<<"$PLAN")
      CONTENT=$(python3 -c 'import json,sys; print(json.load(sys.stdin)[int(sys.argv[1])]["content"])' "$i" <<<"$PLAN")
      "$GH" api graphql -f query="$REACT_MUTATION" -F subjectId="$SUBJ" -F content="$CONTENT" >/dev/null 2>&1 \
        || { log "react failed on $SUBJ"; FAILS=$((FAILS+1)); }
      ;;
    reply)
      TID=$(python3 -c 'import json,sys; print(json.load(sys.stdin)[int(sys.argv[1])]["thread_id"])' "$i" <<<"$PLAN")
      BODY=$(python3 -c 'import json,sys; print(json.load(sys.stdin)[int(sys.argv[1])]["body"])' "$i" <<<"$PLAN")
      "$GH" api graphql -f query="$REPLY_MUTATION" -F threadId="$TID" -F body="$BODY" >/dev/null 2>&1 \
        || { log "reply failed on $TID"; FAILS=$((FAILS+1)); }
      ;;
    resolve)
      TID=$(python3 -c 'import json,sys; print(json.load(sys.stdin)[int(sys.argv[1])]["thread_id"])' "$i" <<<"$PLAN")
      "$GH" api graphql -f query="$RESOLVE_MUTATION" -F threadId="$TID" >/dev/null 2>&1 \
        || { log "resolve failed on $TID"; FAILS=$((FAILS+1)); }
      ;;
  esac
  i=$((i+1))
done

[ "$FAILS" -eq 0 ] || { log "$FAILS operation(s) failed"; exit 1; }
