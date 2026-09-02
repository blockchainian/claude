#!/usr/bin/env bash
# ABOUTME: Parallel codex workstream engine: runs workstreams.txt through codex exec in a worktree
# ABOUTME: pool with per-workstream checks and bounded retries, then merges workstream branches onto the session branch.
set -u

usage() {
  cat <<'EOF'
Usage: execute.sh --workstreams FILE --feature NAME --check CMD
                    [--base BRANCH] [--concurrency N] [--retries N] [--timeout SECS]
                    [--setup CMD] [--spec PATH] [--repo DIR] [--no-push]
                    [--deliver-wait SECS] [--runner exec|daemon]

  workstreams one workstream per line (blank lines and # comments skipped)
              handoff.md must sit beside it (the launch is handed to a fresh session)
  feature     run name; namespaces workstream branches (workstreams/NAME/n) and run state
  check       verify command, run from a worktree root
  base        session branch to deliver onto (default: the branch the repo has
              checked out; when given, it must match the checked-out branch)
  concurrency worktree pool size            (default: CPU count)
  retries     per-workstream retry budget on red   (default: 2)
  timeout     per-codex-invocation seconds   (default: 2400)
  setup       run once per created workstream worktree (e.g. deps provisioning)
  spec        repo-relative path to the shared spec (referenced in workstream
              and merge prompts; e.g. specs/2026-07-22-foo/spec.md)
  repo        repository to operate on       (default: git toplevel of cwd)
  no-push     skip pushing / opening or updating a PR
  deliver-wait max seconds to wait, at merge time, for the session worktree to
              be clean and on the base branch (default: 1800); delivery also
              takes a per-repo lock so concurrent runs merge one at a time
  runner      task runner: daemon or exec       (default: daemon)

Workstreams run in an isolated worktree pool branched from the session branch's
run-start commit. Green workstream branches merge DIRECTLY onto the session
branch in the session worktree once it is clean (uncommitted edits from a
parallel session only delay delivery, up to --deliver-wait); the post-merge
check runs there, and a red check restores the branch to its pre-merge state
(workstream branches kept for autopsy). Local codex review covers the merged workstream delta; the @codex PR
comment covers the whole PR.

Env: EXECUTE_CODEX overrides the codex binary (default: codex).
     EXECUTE_RUNNER sets the task runner (default: daemon).
     EXECUTE_DAEMON_RUNNER overrides the daemon runner executable.
EOF
  exit 1
}

fatal() { echo "codex:execute: FATAL: $*" >&2; exit 1; }
note()  { echo "codex:execute: $*"; }

# ---------- args ----------
DAEMON_RUNNER_OVERRIDDEN=0
[ "${EXECUTE_DAEMON_RUNNER+x}" = x ] && DAEMON_RUNNER_OVERRIDDEN=1
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CODEX="${EXECUTE_CODEX:-codex}"
EXECUTE_RUNNER="${EXECUTE_RUNNER:-daemon}"
EXECUTE_DAEMON_RUNNER="${EXECUTE_DAEMON_RUNNER:-$SCRIPT_DIR/daemon-run.mjs}"

WORKSTREAMS="" BASE="" FEATURE="" CHECK="" SETUP="" SPEC="" REPO="" PUSH=1
CONCURRENCY="" RETRIES=2 TIMEOUT_S=2400 DELIVER_WAIT_S=1800
while [ $# -gt 0 ]; do
  case "$1" in
    --workstreams) WORKSTREAMS="$2"; shift 2 ;;
    --spec) SPEC="$2"; shift 2 ;;
    --base) BASE="$2"; shift 2 ;;
    --feature) FEATURE="$2"; shift 2 ;;
    --check) CHECK="$2"; shift 2 ;;
    --concurrency) CONCURRENCY="$2"; shift 2 ;;
    --retries) RETRIES="$2"; shift 2 ;;
    --timeout) TIMEOUT_S="$2"; shift 2 ;;
    --setup) SETUP="$2"; shift 2 ;;
    --repo) REPO="$2"; shift 2 ;;
    --no-push) PUSH=0; shift ;;
    --deliver-wait) DELIVER_WAIT_S="$2"; shift 2 ;;
    --runner) EXECUTE_RUNNER="$2"; shift 2 ;;
    __workstream) WORKSTREAM_MODE="$2"; shift 2 ;;
    *) usage ;;
  esac
done

case "$EXECUTE_RUNNER" in exec|daemon) ;; *) usage ;; esac

run_task() { # run_task <dir> <last-message-file> <thread-name> <prompt>
  local dir="$1" lastfile="$2" name="$3" prompt="$4"
  if [ "$EXECUTE_RUNNER" = "exec" ]; then
    timeout "$TIMEOUT_S" "$CODEX" exec -C "$dir" -s workspace-write --ephemeral -o "$lastfile" "$prompt"
  else
    "$EXECUTE_DAEMON_RUNNER" -C "$dir" -o "$lastfile" --name "$name" --timeout "$TIMEOUT_S" "$prompt"
  fi
}

# ---------- worker mode ----------
if [ "${WORKSTREAM_MODE:-}" != "" ]; then
  # env supplied by the parent
  idx="$WORKSTREAM_MODE"
  REPO="$EXECUTE_REPO"; BASE_SHA="$EXECUTE_BASE_SHA"; FEATURE="$EXECUTE_FEATURE"; CHECK="$EXECUTE_CHECK"
  SETUP="$EXECUTE_SETUP"; SPEC="$EXECUTE_SPEC"; RETRIES="$EXECUTE_RETRIES"; TIMEOUT_S="$EXECUTE_TIMEOUT"
  RUN_DIR="$EXECUTE_RUN_DIR"; WT_ROOT="$EXECUTE_WT_ROOT"; CONCURRENCY="$EXECUTE_CONCURRENCY"
  LOGD="$RUN_DIR/logs"
  LINE="$(sed -n "${idx}p" "$RUN_DIR/workstreams.txt")"
  BR="workstreams/$FEATURE/$idx"

  status() { # status <result> <attempts> <reason>
    printf '{"workstream": %s, "result": "%s", "attempts": %s, "branch": "%s", "reason": "%s", "line": "%s"}\n' \
      "$idx" "$1" "$2" "$BR" "$3" "$(echo "$LINE" | cut -c1-120 | sed 's/"/\\"/g')" \
      > "$RUN_DIR/status/workstream-$idx.json"
  }

  # acquire a pool slot
  SLOT=""
  while [ -z "$SLOT" ]; do
    s=1
    while [ "$s" -le "$CONCURRENCY" ]; do
      if mkdir "$RUN_DIR/locks/slot-$s" 2>/dev/null; then SLOT="$s"; break; fi
      s=$((s+1))
    done
    [ -z "$SLOT" ] && sleep 0.3
  done
  WT="$WT_ROOT/w$SLOT"

  if [ ! -d "$WT" ]; then
    if ! git -C "$REPO" worktree add --detach "$WT" "$BASE_SHA" > "$LOGD/w$SLOT.create.log" 2>&1; then
      sleep 1  # worktree metadata contention with a sibling worker; retry once
      git -C "$REPO" worktree add --detach "$WT" "$BASE_SHA" >> "$LOGD/w$SLOT.create.log" 2>&1 \
        || { status fail 0 "worktree-create-failed"; rmdir "$RUN_DIR/locks/slot-$SLOT"; exit 0; }
    fi
    if [ -n "$SETUP" ]; then
      if ! (cd "$WT" && eval "$SETUP") > "$LOGD/w$SLOT.setup.log" 2>&1; then
        status fail 0 "setup-failed"
        rmdir "$RUN_DIR/locks/slot-$SLOT"
        exit 0
      fi
    fi
  fi

  git -C "$WT" checkout -q -B "$BR" "$BASE_SHA"
  git -C "$WT" reset -q --hard "$BASE_SHA"
  git -C "$WT" clean -qfd

  SPECHINT=""
  [ -n "$SPEC" ] && SPECHINT=" The shared spec is at $SPEC (repo-relative); read it first."
  PREAMBLE="You are workstream $idx of a parallel batch run. Work ONLY inside this directory. Do not push, do not switch branches, do not touch other workstreams' scopes. The harness runs the check command ($CHECK) and commits for you.$SPECHINT

Workstream: $LINE"

  result="fail"; reason=""; failctx=""; a=0
  while [ "$a" -lt $((RETRIES+1)) ]; do
    a=$((a+1))
    attempt_prefix=""
    attempt_suffix=""
    start_attempt=""
    if [ "$a" -gt 1 ]; then
      attempt_prefix="attempt $a "
      attempt_suffix=" (attempt $a)"
      start_attempt=" (attempt $a)"
    fi
    PROMPT="$PREAMBLE"
    if [ "$a" -gt 1 ]; then
      PROMPT="$PREAMBLE

PREVIOUS ATTEMPT FAILED (attempt $((a-1)) of $((RETRIES+1))): $reason
$failctx
Fix it."
    fi
    note "[workstream $idx] starting${start_attempt}"
    run_task "$WT" "$LOGD/workstream-$idx-a$a.last" "$FEATURE/w$idx a$a" "$PROMPT" \
      > "$LOGD/workstream-$idx-a$a.codex.log" 2>&1
    rc=$?
    if [ "$rc" -eq 124 ]; then
      reason="timeout"; failctx="codex hit the ${TIMEOUT_S}s per-invocation timeout"
      note "[workstream $idx] ${attempt_prefix}red: timeout"; continue
    elif [ "$rc" -ne 0 ]; then
      reason="codex-exit-$rc"; failctx="$(tail -c 3000 "$LOGD/workstream-$idx-a$a.codex.log")"
      note "[workstream $idx] ${attempt_prefix}red: codex exited $rc"; continue
    fi
    commits="$(git -C "$WT" rev-list --count "$BASE_SHA"..HEAD 2>/dev/null || echo 0)"
    if [ "$commits" = "0" ] && [ -z "$(git -C "$WT" status --porcelain)" ]; then
      reason="no-diff"; failctx="codex completed but produced no changes"
      note "[workstream $idx] ${attempt_prefix}red: no diff"; continue
    fi
    (cd "$WT" && eval "$CHECK") > "$LOGD/workstream-$idx-a$a.check.log" 2>&1
    crc=$?
    if [ "$crc" -eq 0 ]; then
      if [ -n "$(git -C "$WT" status --porcelain)" ]; then
        git -C "$WT" add -A
        git -C "$WT" commit -q -m "workstream $idx: $(echo "$LINE" | cut -c1-60)"
      fi
      result="pass"; reason="check-green"
      note "[workstream $idx] PASS${attempt_suffix}"
      break
    fi
    reason="check-failed"; failctx="$(tail -c 3000 "$LOGD/workstream-$idx-a$a.check.log")"
    note "[workstream $idx] ${attempt_prefix}FAIL (exit $crc)"
  done

  if [ "$result" != "pass" ]; then
    # preserve whatever exists on the workstream branch for inspection
    if [ -n "$(git -C "$WT" status --porcelain)" ]; then
      git -C "$WT" add -A
      git -C "$WT" commit -q -m "FAILED workstream $idx (kept for inspection): $reason"
    fi
    note "[workstream $idx] FAILED after $a attempt(s): $reason"
  fi
  status "$result" "$a" "$reason"

  # release the slot with a clean tree
  git -C "$WT" checkout -q --detach
  git -C "$WT" reset -q --hard "$BASE_SHA"
  git -C "$WT" clean -qfd
  rmdir "$RUN_DIR/locks/slot-$SLOT"
  exit 0
fi

# ---------- orchestrating mode ----------
[ -n "$WORKSTREAMS" ] && [ -n "$FEATURE" ] && [ -n "$CHECK" ] || usage
command -v timeout >/dev/null || fatal "timeout(1) not found (brew install coreutils)"
command -v "$CODEX" >/dev/null || fatal "codex binary '$CODEX' not found"
[ -f "$WORKSTREAMS" ] || fatal "workstreams file not found: $WORKSTREAMS"
HANDOFF="$(dirname "$WORKSTREAMS")/handoff.md"
[ -f "$HANDOFF" ] || fatal "handoff.md not found beside workstreams: $HANDOFF (write it, then launch from a fresh session)"

if [ "$EXECUTE_RUNNER" = "daemon" ] && [ "$DAEMON_RUNNER_OVERRIDDEN" = "0" ]; then
  "$CODEX" app-server daemon start >/dev/null 2>&1 \
    || fatal "codex app-server daemon failed to start; use --runner exec as a fallback"
  DAEMON_VERSION="$("$CODEX" app-server daemon version 2>/dev/null)" \
    || fatal "codex app-server daemon is unavailable; use --runner exec as a fallback"
  echo "$DAEMON_VERSION" | grep -q '"status"[[:space:]]*:[[:space:]]*"running"' \
    || fatal "codex app-server daemon is not running; use --runner exec as a fallback"
fi

if [ -z "$REPO" ]; then REPO="$(git rev-parse --show-toplevel 2>/dev/null)" || fatal "not in a git repo and no --repo"; fi
REPO="$(cd "$REPO" && pwd)"
CUR="$(git -C "$REPO" symbolic-ref --short -q HEAD)" \
  || fatal "repo is on a detached HEAD; check out the branch to deliver onto"
[ -z "$BASE" ] && BASE="$CUR"
[ "$BASE" = "$CUR" ] || fatal "repo has '$CUR' checked out but --base is '$BASE'; check out '$BASE' or drop --base"
[ -z "$(git -C "$REPO" status --porcelain)" ] \
  || note "session worktree is dirty; workstreams branch from HEAD and delivery waits for a clean tree"
[ -z "$(git -C "$REPO" branch --list "workstreams/$FEATURE/*")" ] \
  || fatal "leftover workstreams/$FEATURE/* branches exist; delete them or pick a new run name"
BASE_SHA="$(git -C "$REPO" rev-parse HEAD)"

if [ -z "$CONCURRENCY" ]; then
  CONCURRENCY="$( (nproc || sysctl -n hw.ncpu) 2>/dev/null | head -1 )"
  [ -n "$CONCURRENCY" ] || CONCURRENCY=4
fi

GITDIR="$(git -C "$REPO" rev-parse --absolute-git-dir)"
RUN_DIR="$GITDIR/codex-execute/$FEATURE"
WT_ROOT="$(dirname "$REPO")/.codex-execute-$FEATURE"
rm -rf "$RUN_DIR"
mkdir -p "$RUN_DIR/logs" "$RUN_DIR/status" "$RUN_DIR/locks" "$WT_ROOT"

grep -v '^[[:space:]]*$' "$WORKSTREAMS" | grep -v '^[[:space:]]*#' > "$RUN_DIR/workstreams.txt"
N="$(wc -l < "$RUN_DIR/workstreams.txt" | tr -d ' ')"
[ "$N" -gt 0 ] || fatal "no workstreams in $WORKSTREAMS"
[ "$CONCURRENCY" -gt "$N" ] && CONCURRENCY="$N"

note "feature=$FEATURE base=$BASE workstreams=$N pool=$CONCURRENCY retries=$RETRIES timeout=${TIMEOUT_S}s"
note "runner=$EXECUTE_RUNNER"
note "delivering onto branch '$BASE' in $REPO"
note "logs: $RUN_DIR/logs  status: $RUN_DIR/status"

SELF="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
export EXECUTE_REPO="$REPO" EXECUTE_BASE_SHA="$BASE_SHA" EXECUTE_FEATURE="$FEATURE" EXECUTE_CHECK="$CHECK"
export EXECUTE_SETUP="$SETUP" EXECUTE_SPEC="$SPEC" EXECUTE_RETRIES="$RETRIES" EXECUTE_TIMEOUT="$TIMEOUT_S"
export EXECUTE_RUN_DIR="$RUN_DIR" EXECUTE_WT_ROOT="$WT_ROOT" EXECUTE_CONCURRENCY="$CONCURRENCY"
export EXECUTE_CODEX="$CODEX" EXECUTE_RUNNER EXECUTE_DAEMON_RUNNER

# ---------- phase: execute ----------
seq 1 "$N" | xargs -n1 -P "$CONCURRENCY" "$SELF" __workstream

PASSED=""; FAILED=""
i=1
while [ "$i" -le "$N" ]; do
  if grep -q '"result": "pass"' "$RUN_DIR/status/workstream-$i.json" 2>/dev/null; then
    PASSED="$PASSED $i"
  else
    FAILED="$FAILED $i"
  fi
  i=$((i+1))
done
EXECUTION_SUMMARY="PASS [${PASSED# }]"
[ -n "$FAILED" ] && EXECUTION_SUMMARY="$EXECUTION_SUMMARY FAIL [${FAILED# }]"
note "$EXECUTION_SUMMARY"

# ---------- delivery lock + clean-tree wait ----------
# One delivery at a time per repo: the lock is a directory (atomic mkdir; macOS has no flock)
# holding the owner pid, held from merge through push and released on exit.
DELIVER_LOCK="$(git -C "$REPO" rev-parse --absolute-git-dir)/codex-execute/deliver.lock"
LOCK_HELD=0
release_delivery_lock() { [ "$LOCK_HELD" = "1" ] && rm -rf "$DELIVER_LOCK"; LOCK_HELD=0; }
trap release_delivery_lock EXIT
acquire_delivery_lock() { # acquire_delivery_lock <max-seconds>; returns 1 on timeout
  local waited=0 owner
  while ! mkdir "$DELIVER_LOCK" 2>/dev/null; do
    owner="$(cat "$DELIVER_LOCK/pid" 2>/dev/null)"
    if [ -n "$owner" ] && ! kill -0 "$owner" 2>/dev/null; then
      rm -rf "$DELIVER_LOCK"; continue   # stale lock from a dead run
    fi
    [ "$waited" -ge "$1" ] && return 1
    [ "$waited" = 0 ] && note "[merge] waiting for delivery lock held by pid ${owner:-?}"
    sleep 5; waited=$((waited+5))
  done
  echo $$ > "$DELIVER_LOCK/pid"; LOCK_HELD=1
}
tree_dirt() { git -C "$REPO" status --porcelain | head -5 | awk '{print $2}' | tr '\n' ' '; }
wait_for_clean_tree() { # wait_for_clean_tree <max-seconds>; sets MERGE_BLOCKED on timeout
  local waited=0 announced=0
  while :; do
    if [ "$(git -C "$REPO" symbolic-ref --short -q HEAD)" != "$BASE" ]; then
      MERGE_BLOCKED="session worktree switched off '$BASE' during the run"; return
    fi
    [ -z "$(git -C "$REPO" status --porcelain)" ] && { [ "$announced" = 1 ] && note "[merge] session worktree clean; resuming"; return; }
    if [ "$waited" -ge "$1" ]; then
      MERGE_BLOCKED="session worktree stayed dirty for ${1}s ($(tree_dirt))"; return
    fi
    [ "$announced" = 0 ] && { note "[merge] waiting for a clean session worktree (dirty: $(tree_dirt))"; announced=1; }
    sleep 5; waited=$((waited+5))
  done
}

# ---------- phase: merge (directly onto the session branch) ----------
MERGED=""; MERGE_FAILED=""; POST="skip"; RESTORED=false; MERGE_BLOCKED=""
PRE_MERGE=""
if [ -n "$PASSED" ]; then
  if acquire_delivery_lock "$DELIVER_WAIT_S"; then
    wait_for_clean_tree "$DELIVER_WAIT_S"
  else
    MERGE_BLOCKED="delivery lock held for ${DELIVER_WAIT_S}s by another run"
  fi
fi

if [ -n "$MERGE_BLOCKED" ]; then
  note "[merge] BLOCKED: $MERGE_BLOCKED; workstream branches kept"
elif [ -n "$PASSED" ]; then
  PRE_MERGE="$(git -C "$REPO" rev-parse HEAD)"
  git -C "$REPO" update-ref "refs/codex-execute/$FEATURE/pre-merge" "$PRE_MERGE"
  echo "$PRE_MERGE" > "$RUN_DIR/pre-merge.sha"

  for idx in $PASSED; do
    BR="workstreams/$FEATURE/$idx"
    if git -C "$REPO" merge -q --no-ff -m "merge workstream $idx" "$BR" > "$RUN_DIR/logs/merge-$idx.log" 2>&1; then
      MERGED="$MERGED $idx"
      note "[merge] workstream $idx MERGED"
      continue
    fi
    CONFLICTED="$(git -C "$REPO" diff --name-only --diff-filter=U | tr '\n' ' ')"
    note "[merge] workstream $idx CONFLICT: $CONFLICTED-> resolving with codex"
    MPROMPT="You are resolving a git merge conflict in this repository. Branch 'workstreams/$FEATURE/$idx' is being merged into '$BASE'. Conflicted files: $CONFLICTED
Read ${SPEC:-spec.md at the repo root, if present,} for intent. Resolve every conflict so BOTH sides' intent is preserved, then 'git add' each resolved file. Do not commit, do not push."
    run_task "$REPO" "$RUN_DIR/logs/merge-$idx.last" "$FEATURE/merge-$idx" "$MPROMPT" \
      >> "$RUN_DIR/logs/merge-$idx.log" 2>&1
    if [ -z "$(git -C "$REPO" diff --name-only --diff-filter=U)" ] \
       && git -C "$REPO" commit -q --no-edit >> "$RUN_DIR/logs/merge-$idx.log" 2>&1; then
      MERGED="$MERGED $idx"
      note "[merge] workstream $idx MERGED (conflict resolved)"
    else
      git -C "$REPO" merge --abort >> "$RUN_DIR/logs/merge-$idx.log" 2>&1
      MERGE_FAILED="$MERGE_FAILED $idx"
      note "[merge] workstream $idx merge FAILED; excluded (branch kept)"
    fi
  done

  # post-merge check (conflict resolutions are code no workstream gate covered)
  if [ -n "$MERGED" ]; then
    if (cd "$REPO" && eval "$CHECK") > "$RUN_DIR/logs/post-merge-check.log" 2>&1; then
      POST="pass"; note "[merge] PASS"
    else
      POST="fail"
      git -C "$REPO" reset -q --keep "$PRE_MERGE" && RESTORED=true
      note "[merge] post-merge check RED — session branch restored; workstream branches kept (log: $RUN_DIR/logs/post-merge-check.log)"
    fi
  fi
fi
# The branch is now gated (merged + checked, or restored); review is read-only against
# the recorded pre-merge sha and push handles a moved remote, so other runs may deliver.
release_delivery_lock

# ---------- cleanup: workstream worktrees + merged/empty workstream branches ----------
s=1
while [ "$s" -le "$CONCURRENCY" ]; do
  [ -d "$WT_ROOT/w$s" ] && git -C "$REPO" worktree remove --force "$WT_ROOT/w$s" >/dev/null 2>&1
  s=$((s+1))
done
rmdir "$WT_ROOT" 2>/dev/null
i=1
while [ "$i" -le "$N" ]; do
  BR="workstreams/$FEATURE/$i"
  if git -C "$REPO" rev-parse --verify -q "refs/heads/$BR" >/dev/null; then
    keep=0
    case " $MERGE_FAILED " in *" $i "*) keep=1 ;; esac  # merge-failed: keep for inspection
    case " $FAILED " in *" $i "*)
      [ "$(git -C "$REPO" rev-list --count "$BASE_SHA".."$BR")" != "0" ] && keep=1 ;;
    esac
    if [ "$POST" != "pass" ]; then
      # nothing (or a red result) landed on the session branch — the workstream
      # branches are the only copy of the work
      case " $PASSED " in *" $i "*) keep=1 ;; esac
    fi
    [ "$keep" = "0" ] && git -C "$REPO" branch -q -D "$BR"
  fi
  i=$((i+1))
done

# ---------- review (local codex review of the merged workstream delta) ----------
REVIEW="skip"
if [ "$POST" = "pass" ]; then
  note "[review] merged workstreams vs pre-merge"
  if timeout "$TIMEOUT_S" "$CODEX" exec -C "$REPO" -c model_reasoning_effort=high \
       -o "$RUN_DIR/logs/review.md" \
       review --base "$PRE_MERGE" > "$RUN_DIR/logs/review.log" 2>&1; then
    REVIEW="done"; note "[review] result: $RUN_DIR/logs/review.md"
  else
    REVIEW="failed"; note "[review] codex review failed (see $RUN_DIR/logs/review.log)"
  fi
fi

# ---------- deliver: push the session branch; the PR is FROM it ----------
PUSHED=false; DELIVERED="none"
if [ "$POST" = "pass" ] && [ "$PUSH" = "1" ]; then
  # push_base: push; on a non-fast-forward (another run or session pushed meanwhile)
  # fetch, merge the remote tip in, re-run the gate on the combined branch, push once more.
  # A conflicting or red combination fails the push and is left for a human.
  push_base() {
    git -C "$REPO" push -q -u origin "$BASE" > "$RUN_DIR/logs/push.log" 2>&1 && return 0
    grep -qiE 'non-fast-forward|fetch first|rejected' "$RUN_DIR/logs/push.log" || return 1
    note "push rejected (remote moved); merging origin/$BASE, re-running check, retrying"
    git -C "$REPO" fetch -q origin "$BASE" >> "$RUN_DIR/logs/push.log" 2>&1 || return 1
    if ! git -C "$REPO" merge -q --no-edit "origin/$BASE" >> "$RUN_DIR/logs/push.log" 2>&1; then
      git -C "$REPO" merge --abort >> "$RUN_DIR/logs/push.log" 2>&1; return 1
    fi
    (cd "$REPO" && eval "$CHECK") > "$RUN_DIR/logs/push-recheck.log" 2>&1 || return 1
    git -C "$REPO" push -q -u origin "$BASE" >> "$RUN_DIR/logs/push.log" 2>&1
  }
  if push_base; then
    PUSHED=true; DELIVERED="$BASE"; note "pushed to origin"
    if gh pr view "$BASE" > "$RUN_DIR/logs/pr.log" 2>&1; then
      if gh pr comment "$BASE" --body "@codex review" >> "$RUN_DIR/logs/pr.log" 2>&1; then
        note "existing PR updates; GitHub review requested"
      else
        note "existing PR updates; @codex comment failed (see $RUN_DIR/logs/pr.log)"
      fi
    else
      BODY="codex:execute run '$FEATURE': $N workstreams, passed:[${PASSED# }] failed:[${FAILED# }] merge-failed:[${MERGE_FAILED# }]. Post-merge check: $POST. Codex review: $REVIEW (findings in run logs)."
      if gh pr create --head "$BASE" --title "execute: $FEATURE" --body "$BODY" >> "$RUN_DIR/logs/pr.log" 2>&1; then
        note "PR is $(tail -1 "$RUN_DIR/logs/pr.log")"
        if gh pr comment "$BASE" --body "@codex review" >> "$RUN_DIR/logs/pr.log" 2>&1; then
          note "GitHub review requested"
        else
          note "no @codex comment posted (gh failed); local review already ran"
        fi
      else
        note "no PR created (gh failed, or '$BASE' is the default branch); see $RUN_DIR/logs/pr.log"
      fi
    fi
  else
    note "push failed (see $RUN_DIR/logs/push.log)"
  fi
fi

# ---------- summary ----------
NP=0; NF=0; NM=0; NMF=0
for x in $PASSED; do NP=$((NP+1)); done
for x in $FAILED; do NF=$((NF+1)); done
for x in $MERGED; do NM=$((NM+1)); done
for x in $MERGE_FAILED; do NMF=$((NMF+1)); done
printf '{"feature": "%s", "base": "%s", "workstreams": %s, "passed": %s, "failed": %s, "merged": %s, "merge_failed": %s, "post_merge_check": "%s", "review": "%s", "pushed": %s, "delivered_to": "%s", "restored": %s, "merge_blocked": "%s"}\n' \
  "$FEATURE" "$BASE" "$N" "$NP" "$NF" "$NM" "$NMF" "$POST" "$REVIEW" "$PUSHED" "$DELIVERED" "$RESTORED" "$MERGE_BLOCKED" \
  > "$RUN_DIR/status/summary.json"
SUMMARY="feature=$FEATURE base=$BASE workstreams=$N"
[ "$NP" -gt 0 ] && SUMMARY="$SUMMARY pass=$NP"
[ "$NF" -gt 0 ] && SUMMARY="$SUMMARY fail=$NF"
[ "$NM" -gt 0 ] && SUMMARY="$SUMMARY merged=$NM"
[ "$NMF" -gt 0 ] && SUMMARY="$SUMMARY merge=fail"
[ -n "$MERGE_BLOCKED" ] && SUMMARY="$SUMMARY merge=blocked"
case "$POST" in
  pass) SUMMARY="$SUMMARY post-merge=pass" ;;
  fail) SUMMARY="$SUMMARY post-merge=fail" ;;
esac
[ "$RESTORED" = "true" ] && SUMMARY="$SUMMARY restored"
case "$REVIEW" in
  done) SUMMARY="$SUMMARY reviewed" ;;
  failed) SUMMARY="$SUMMARY review=failed" ;;
esac
[ "$PUSHED" = "true" ] && SUMMARY="$SUMMARY pushed"
note "summary: $SUMMARY"

if [ -n "$MERGE_BLOCKED" ]; then exit 1; fi
if [ "$POST" = "fail" ]; then exit 1; fi
if [ "$PUSH" = "1" ] && [ "$POST" = "pass" ] && [ "$PUSHED" != "true" ]; then exit 1; fi
if [ -n "$FAILED" ] || [ -n "$MERGE_FAILED" ]; then exit 2; fi
exit 0
