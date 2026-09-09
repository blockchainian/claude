#!/usr/bin/env bash
# ABOUTME: Drives bounded rounds of the Codex fix-pr skill over one GitHub pull request,
# ABOUTME: routing UI threads to Claude Code and reporting rounds, pushes and staging deploys as JSON.
set -u

usage() {
  cat <<'EOF'
Usage: autofix-pr.sh --pr NUMBER [--repo DIR] [--max-rounds N] [--production]
                 [--ux-file PATH] [--wait SECS] [--poll SECS] [--timeout SECS]

  pr           pull request number in the repo's origin
  repo         repository to operate on        (default: git toplevel of cwd)
  max-rounds   review/fix rounds to run        (default: 2)
  production   let the Codex skill deploy production after staging passes; without it
               the skill stops after staging verification and production stays with the caller
  ux-file      append each thread id the Codex skill routes to Claude Code, one per line,
               as soon as it is marked — while the round is still running
  wait         max seconds to wait for a review newer than the PR head (default: 900)
  poll         seconds between those checks    (default: 10)
  timeout      per-Codex-invocation seconds    (default: 3600)

Each round waits for a submitted review, review comment, or PR reaction newer than the
current branch head (the branch ref, since the PR head can lag a push), runs the Codex fix-pr skill as a daemon thread named '<pr>/fix-pr r<n>', then
re-reads the PR. After a push the next round waits for the reviewer to react to the new
head before reading the threads. The run ends when a reviewed head has no unresolved
must-fix thread, the rounds are exhausted, or no review arrives in time, and prints one
flat JSON object.

Exit: 0 = no unresolved must-fix threads; 2 = threads remain; 1 = fatal.

Env: FIXPR_GH overrides the gh binary (default: gh).
     FIXPR_CODEX overrides the codex binary (default: codex).
     FIXPR_DAEMON_RUNNER overrides the daemon runner executable.
EOF
  exit 1
}

fatal() { echo "codex:autofix-pr: FATAL: $*" >&2; exit 1; }
note()  { echo "codex:autofix-pr: $*" >&2; }

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
GH="${FIXPR_GH:-gh}"
CODEX="${FIXPR_CODEX:-codex}"
DAEMON_RUNNER_OVERRIDDEN=0
[ "${FIXPR_DAEMON_RUNNER+x}" = x ] && DAEMON_RUNNER_OVERRIDDEN=1
DAEMON_RUNNER="${FIXPR_DAEMON_RUNNER:-$SCRIPT_DIR/../execute/daemon-run.mjs}"

PR="" REPO="" MAX_ROUNDS=2 PRODUCTION=0 UX_FILE="" WAIT_S=900 POLL_S=10 TIMEOUT_S=3600
while [ $# -gt 0 ]; do
  case "$1" in
    --pr) PR="$2"; shift 2 ;;
    --repo) REPO="$2"; shift 2 ;;
    --max-rounds) MAX_ROUNDS="$2"; shift 2 ;;
    --production) PRODUCTION=1; shift ;;
    --ux-file) UX_FILE="$2"; shift 2 ;;
    --wait) WAIT_S="$2"; shift 2 ;;
    --poll) POLL_S="$2"; shift 2 ;;
    --timeout) TIMEOUT_S="$2"; shift 2 ;;
    *) usage ;;
  esac
done

[ -n "$PR" ] || usage
case "$PR" in *[!0-9]*) usage ;; esac
[ "$MAX_ROUNDS" -ge 1 ] 2>/dev/null || usage
[ "$POLL_S" -ge 1 ] 2>/dev/null || usage

command -v jq >/dev/null || fatal "jq not found"
command -v "$GH" >/dev/null || fatal "gh binary '$GH' not found"

if [ -z "$REPO" ]; then REPO="$(git rev-parse --show-toplevel 2>/dev/null)" || fatal "not in a git repo and no --repo"; fi
REPO="$(cd "$REPO" && pwd)" || fatal "repo not found: $REPO"

if [ "$DAEMON_RUNNER_OVERRIDDEN" = "0" ]; then
  command -v "$CODEX" >/dev/null || fatal "codex binary '$CODEX' not found"
  # The daemon keeps the cwd it was started from for its whole life; start it from $HOME so a
  # deleted worktree cannot break thread/start for every client attached to it.
  (cd "$HOME" && "$CODEX" app-server daemon start) >/dev/null 2>&1 \
    || fatal "codex app-server daemon failed to start"
  "$CODEX" app-server daemon version 2>/dev/null | grep -q '"status"[[:space:]]*:[[:space:]]*"running"' \
    || fatal "codex app-server daemon is not running"
fi

WORK="$(mktemp -d "${TMPDIR:-/tmp}/codex-autofix-pr.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

gh_api() { (cd "$REPO" && "$GH" api "$@"); }

# The Codex skill replies and resolves as this account, and GitHub wraps each reply in a review;
# those must not pass for the reviewer reacting to a push.
ME="$(gh_api user | jq -r '.login // ""' 2>/dev/null)"

UX_LABEL="claude-code-ux"
UX_MARKER="[UX — Claude Code]"

# The head is read from the branch ref, not from the PR: GitHub's pull-request head can lag a
# push by minutes, and a review matched against the lagging head would pass for a branch head
# the reviewer has not seen.
HEAD_SHA="" HEAD_TIME="" HEAD_REF="" HAS_UX_LABEL=0
refresh_pr() {
  gh_api "repos/{owner}/{repo}/pulls/$PR" > "$WORK/pr.json" || fatal "cannot read PR #$PR"
  HEAD_REF="$(jq -r '.head.ref' "$WORK/pr.json")"
  HEAD_SHA="$(gh_api "repos/{owner}/{repo}/git/ref/heads/$HEAD_REF" | jq -r '.object.sha' 2>/dev/null)"
  [ -n "$HEAD_SHA" ] && [ "$HEAD_SHA" != "null" ] || HEAD_SHA="$(jq -r '.head.sha' "$WORK/pr.json")"
  OWNER="$(jq -r '.base.repo.owner.login' "$WORK/pr.json")"
  NAME="$(jq -r '.base.repo.name' "$WORK/pr.json")"
  HAS_UX_LABEL="$(jq -r --arg l "$UX_LABEL" '[.labels[]?.name] | index($l) | if . == null then 0 else 1 end' "$WORK/pr.json")"
  gh_api "repos/{owner}/{repo}/commits/$HEAD_SHA" > "$WORK/commit.json" || fatal "cannot read commit $HEAD_SHA"
  HEAD_TIME="$(jq -r '.commit.committer.date' "$WORK/commit.json")"
}

THREADS_QUERY='query($owner: String!, $name: String!, $pr: Int!) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $pr) {
      reviewThreads(first: 100) {
        nodes { id isResolved isOutdated path comments(first: 50) { nodes { body } } }
      }
    }
  }
}'

# An unresolved thread is a UX thread when the PR carries the claude-code-ux label and the
# Codex skill's reply marker is on it; a config thread when it touches Claude Code config,
# which the Codex skill never edits. Everything else unresolved is must-fix work.
classify() {
  gh_api graphql -F owner="$OWNER" -F name="$NAME" -F pr="$PR" -f query="$THREADS_QUERY" \
    > "$WORK/threads.json" || fatal "cannot read the review threads of PR #$PR"
  jq -r --arg marker "$UX_MARKER" --argjson label "$HAS_UX_LABEL" '
    [.data.repository.pullRequest.reviewThreads.nodes[]
     | select(.isResolved | not)
     | {id, path: (.path // ""), body: ([.comments.nodes[].body] | join("\n"))}]
    | map(. + {kind:
        (if $label == 1 and (.body | contains($marker)) then "ux"
         elif (.path + "\n" + .body) | test("\\.claude/|CLAUDE\\.md") then "config"
         else "remaining" end)})
    | .[] | "\(.kind)\t\(.id)"' "$WORK/threads.json" > "$WORK/classified" \
    || fatal "cannot classify the review threads of PR #$PR"
  awk -F'\t' '$1 == "ux" {print $2}' "$WORK/classified" > "$WORK/ux"
  awk -F'\t' '$1 == "config" {print $2}' "$WORK/classified" > "$WORK/config"
  awk -F'\t' '$1 == "remaining" {print $2}' "$WORK/classified" > "$WORK/remaining"
}

# The reviewer's reaction to the head, in one GraphQL query per poll: GraphQL has its own
# rate budget, so several engines polling at once do not eat the REST budget Codex uses.
WAIT_QUERY='query($owner: String!, $name: String!, $pr: Int!) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $pr) {
      reviews(last: 50) { nodes { submittedAt author { login } commit { oid } } }
      reviewThreads(last: 100) { nodes { comments(last: 10) { nodes { createdAt author { login } } } } }
      reactions(last: 50) { nodes { createdAt content user { login } } }
    }
  }
}'

# Bounded poll: at most WAIT_S/POLL_S checks for a review submitted on the head commit, or a
# review comment or thumbs-up on the PR newer than the head's commit time, from anyone but this
# account. A review records the commit it was made on, so it is matched by SHA; a comment's
# commit follows the moving head and a reaction has none, so those two stay time-based. A clean
# Codex re-review leaves no review at all: the bot reacts with a thumbs-up on the PR; its "eyes"
# reaction only means the review is in progress.
wait_for_review() {
  local checks=$((WAIT_S / POLL_S)) i=0 fresh
  [ "$checks" -lt 1 ] && checks=1
  while [ "$i" -lt "$checks" ]; do
    i=$((i+1))
    fresh="$(gh_api graphql -F owner="$OWNER" -F name="$NAME" -F pr="$PR" -f query="$WAIT_QUERY" \
      | jq -r --arg t "$HEAD_TIME" --arg head "$HEAD_SHA" --arg me "$ME" '
          .data.repository.pullRequest as $p
          | ([$p.reviews.nodes[] | select(.submittedAt != null and .commit.oid == $head and (.author.login // "") != $me)]
             + [$p.reviewThreads.nodes[].comments.nodes[] | select(.createdAt > $t and (.author.login // "") != $me)]
             + [$p.reactions.nodes[] | select(.content == "THUMBS_UP" and .createdAt > $t and (.user.login // "") != $me)])
          | length' 2>/dev/null)"
    if [ "${fresh:-0}" -gt 0 ] 2>/dev/null; then return 0; fi
    [ "$i" -lt "$checks" ] && sleep "$POLL_S"
  done
  return 1
}

STAGING_CLAUSE="This invocation is staging-only: deploy staging, verify it, and stop before production, reporting the staging SHA."
[ "$PRODUCTION" = "1" ] && STAGING_CLAUSE="Deploy staging, verify it, then deploy production as the skill prescribes."
REPORT_CLAUSE="Quote every deploy-staging.sh JSON result line verbatim in your final message."
LANES_CLAUSE="Threads that already carry the [UX — Claude Code] reply belong to Claude Code, which fixes them in parallel on this PR: do not modify, reply to, or resolve them. Before every push, rebase onto the remote PR branch, since the other lane may have pushed."

run_round() { # run_round <round>
  local round="$1" prompt
  prompt="Use your fix-pr skill on pull request #$PR of $OWNER/$NAME, checked out at $REPO. $STAGING_CLAUSE $REPORT_CLAUSE $LANES_CLAUSE"
  note "[round $round] invoking the Codex fix-pr skill"
  "$DAEMON_RUNNER" -C "$REPO" -o "$WORK/last-$round.txt" --name "$PR/fix-pr r$round" \
    --timeout "$TIMEOUT_S" "$prompt" > "$WORK/round-$round.log" 2>&1 &
  local daemon=$!
  # While the Codex skill works, re-read the threads so a thread it routes to Claude Code
  # reaches the UX lane now, not when the round ends.
  while kill -0 "$daemon" 2>/dev/null; do
    stream_ux
    sleep "$POLL_S"
  done
  wait "$daemon"
  stream_ux
}

# Append every newly marked UX thread id to UX_FILE and say so on stderr.
stream_ux() {
  [ -n "$UX_FILE" ] || return 0
  refresh_pr
  classify
  touch "$WORK/ux-seen"
  while IFS= read -r id; do
    [ -n "$id" ] || continue
    grep -qx "$id" "$WORK/ux-seen" && continue
    echo "$id" >> "$WORK/ux-seen"
    echo "$id" >> "$UX_FILE"
    note "ux thread routed to Claude Code: $id"
  done < "$WORK/ux"
}

# The Codex skill quotes each deploy-staging.sh result line; the staging SHA is its "sha" field.
collect_staging() { # collect_staging <last-message-file>
  [ -f "$1" ] || return 0
  grep -o '{.*}' "$1" | while IFS= read -r line; do
    printf '%s\n' "$line" | jq -r 'select(type == "object" and .target == "staging") | .sha // empty' 2>/dev/null
  done >> "$WORK/staging"
}

: > "$WORK/pushed"
: > "$WORK/staging"
refresh_pr

# Threads are read only from a head the reviewer has reacted to; a head pushed after the last
# review is awaiting review, not clean.
ROUNDS=0 AWAITING_REVIEW=false
while :; do
  if ! wait_for_review; then
    note "no review newer than $HEAD_SHA within ${WAIT_S}s; stopping"
    AWAITING_REVIEW=true
  fi
  classify
  [ "$AWAITING_REVIEW" = false ] || break
  [ -s "$WORK/remaining" ] || break
  [ "$ROUNDS" -lt "$MAX_ROUNDS" ] || break
  ROUNDS=$((ROUNDS+1))
  PREV_SHA="$HEAD_SHA"
  run_round "$ROUNDS"
  collect_staging "$WORK/last-$ROUNDS.txt"
  refresh_pr
  [ "$HEAD_SHA" != "$PREV_SHA" ] && echo "$HEAD_SHA" >> "$WORK/pushed"
done

jq -n --argjson rounds "$ROUNDS" --argjson awaiting "$AWAITING_REVIEW" \
  --rawfile pushed "$WORK/pushed" --rawfile staging "$WORK/staging" \
  --rawfile ux "$WORK/ux" --rawfile config "$WORK/config" --rawfile remaining "$WORK/remaining" \
  'def lines: split("\n") | map(select(length > 0))
     | reduce .[] as $item ([]; if index($item) then . else . + [$item] end);
   {rounds: $rounds, awaiting_review: $awaiting, pushed: ($pushed | lines),
    staging_deploys: ($staging | lines), ux_threads: ($ux | lines),
    config_threads: ($config | lines), remaining: ($remaining | lines)}' -c

[ -s "$WORK/remaining" ] && exit 2
exit 0
