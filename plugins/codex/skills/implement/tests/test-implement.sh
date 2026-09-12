#!/usr/bin/env bash
# ABOUTME: End-to-end test for implement.sh using a fixture git repo and the stub codex CLI.
# ABOUTME: Covers pass, retry, hang, no-diff, conflicts, session-branch delivery, restore, guards, brief-format checks.
set -u

HERE="$(cd "$(dirname "$0")" && pwd)"
IMPLEMENT="$HERE/../implement.sh"

if [ -z "${TEST_IMPLEMENT_RUNNER:-}" ]; then
  batch_rc=0
  for test_runner in exec daemon; do
    echo "==== runner: $test_runner ===="
    TEST_IMPLEMENT_RUNNER="$test_runner" "$0" || batch_rc=1
  done
  exit "$batch_rc"
fi

SCRATCH="$(mktemp -d /tmp/codex-implement-test.XXXXXX)"
trap 'rm -rf "$SCRATCH"' EXIT

FAILS=0
assert() { # assert <desc> <cmd...>
  local desc="$1"; shift
  if "$@" >/dev/null 2>&1; then
    echo "PASS: $desc"
  else
    echo "FAIL: $desc  [cmd: $*]"
    FAILS=$((FAILS+1))
  fi
}
assert_eq() { # assert_eq <desc> <expected> <actual>
  if [ "$2" = "$3" ]; then
    echo "PASS: $1"
  else
    echo "FAIL: $1  [expected '$2' got '$3']"
    FAILS=$((FAILS+1))
  fi
}

# ---------- skill metadata ----------
# Claude Code prefixes the plugin name itself, so the frontmatter name carries no namespace.
SKILL_NAME="$(awk '/^name:/{sub(/^name: */, ""); gsub(/"/, ""); print; exit}' "$HERE/../SKILL.md")"
assert_eq "SKILL.md name resolves to /codex:implement" "implement" "$SKILL_NAME"

# ---------- fixture ----------
export STUB_DIR="$SCRATCH/stub"
mkdir -p "$STUB_DIR"
# inject the stub via IMPLEMENT_CODEX: a single path, immune to PATH quirks (e.g. colons in dir names)
export IMPLEMENT_CODEX="$HERE/stub-codex/codex"
if [ "$TEST_IMPLEMENT_RUNNER" = "exec" ]; then
  export IMPLEMENT_RUNNER=exec
  unset IMPLEMENT_DAEMON_RUNNER
else
  unset IMPLEMENT_RUNNER
  STUB_SPACED="$SCRATCH/stub dir"
  mkdir -p "$STUB_SPACED/stub-daemon-runner"
  cp "$HERE/stub-scenarios.sh" "$STUB_SPACED/"
  cp "$HERE/stub-daemon-runner/daemon-run" "$STUB_SPACED/stub-daemon-runner/"
  export IMPLEMENT_DAEMON_RUNNER="$STUB_SPACED/stub-daemon-runner/daemon-run"
fi
STUB_BIN="$SCRATCH/bin"
mkdir -p "$STUB_BIN"
cat > "$STUB_BIN/gh" <<'EOF'
#!/bin/sh
case "$1 $2" in
  "pr view") [ "${GH_VIEW_OK:-0}" = "1" ] || exit 1 ;;
  "pr create") echo "https://github.com/example/app/pull/42" ;;
  "pr comment") touch "${STUB_DIR:?}/gh-comment" ;;
  *) exit 1 ;;
esac
EOF
chmod +x "$STUB_BIN/gh"
export PATH="$STUB_BIN:$PATH"

FIX="$SCRATCH/repo"
ORIGIN="$SCRATCH/origin.git"
git init -q -b main "$FIX"
git -C "$FIX" config user.email test@test && git -C "$FIX" config user.name test
cat > "$FIX/check.sh" <<'EOF'
#!/bin/sh
for f in a.txt b.txt c.txt; do
  [ -f "$f" ] || continue
  case "$(cat "$f")" in
    ok*) ;;
    *) echo "BAD content in $f"; exit 1 ;;
  esac
done
# d and e pass alone but fail combined: drives the post-merge-red scenario
if [ -f d.txt ] && [ -f e.txt ]; then echo "d.txt and e.txt conflict"; exit 1; fi
exit 0
EOF
echo "shared spec for workstreams" > "$FIX/spec.md"
cat > "$FIX/workstreams.txt" <<'EOF'
WS-OK write a.txt per spec.md
WS-FAILONCE write b.txt per spec.md
WS-HANG never finishes
WS-NOOP produces no diff
WS-C1 write c.txt (variant 1)
WS-C2 write c.txt (variant 2)
EOF
git -C "$FIX" add -A && git -C "$FIX" commit -qm "base"
git init -q --bare "$ORIGIN"
git -C "$FIX" remote add origin "$ORIGIN"
git -C "$FIX" push -q origin main

BASE0="$(git -C "$FIX" rev-parse main)"

# ---------- runner validation ----------
set +e
"$IMPLEMENT" --runner bogus > "$SCRATCH/bogus.log" 2>&1
BOGUS_RC=$?
set -e 2>/dev/null || true
assert_eq "bogus runner exits 1" 1 "$BOGUS_RC"
assert "bogus runner prints usage" grep -q '^Usage:' "$SCRATCH/bogus.log"

# ---------- daemon start cwd ----------
# The shared app-server daemon inherits the cwd of whoever starts it, and every later codex TUI
# attaches to it. Starting it from a worktree that is later deleted breaks thread/start for all
# clients, so implement.sh must start it from $HOME.
if [ "$TEST_IMPLEMENT_RUNNER" = "daemon" ]; then
  mkdir -p "$SCRATCH/not-a-repo"
  set +e
  (cd "$SCRATCH/not-a-repo" && env -u IMPLEMENT_DAEMON_RUNNER "$IMPLEMENT" --workstreams "$FIX/workstreams.txt" \
    --feature feat-cwd --check "sh ./check.sh") > "$SCRATCH/daemon-cwd.log" 2>&1
  set -e 2>/dev/null || true
  assert "implement.sh started the daemon" test -f "$STUB_DIR/daemon-start-cwd"
  assert_eq "daemon is started from \$HOME, not the launching directory" "$HOME" "$(cat "$STUB_DIR/daemon-start-cwd" 2>/dev/null)"
  assert "implement.sh stopped at the repo check after the daemon block" grep -q 'not in a git repo' "$SCRATCH/daemon-cwd.log"
fi

# ---------- scenario 1: partial run, delivery onto the session branch ----------
set +e
"$IMPLEMENT" --workstreams "$FIX/workstreams.txt" --feature feat-x \
  --check "sh ./check.sh" --concurrency 2 --retries 1 --timeout 3 \
  --repo "$FIX" > "$SCRATCH/run.log" 2>&1
RC=$?
set -e 2>/dev/null || true

echo "---- implement.sh exit=$RC (log: $SCRATCH/run.log) ----"

# ---------- assertions ----------
assert_eq "exit code 2 (partial: failed workstreams, session branch green)" 2 "$RC"

WT_ROOT="$(dirname "$FIX")/.codex-implement-feat-x"

# merged content landed on the session branch, in the session worktree
assert_eq "still on main" "main" "$(git -C "$FIX" symbolic-ref --short HEAD)"
assert_eq "a.txt from workstream 1" "ok-ws1" "$(cat "$FIX/a.txt" 2>/dev/null)"
assert_eq "b.txt from workstream 2 retry" "ok-ws2" "$(cat "$FIX/b.txt" 2>/dev/null)"
assert_eq "c.txt conflict resolved by codex" "ok-merged" "$(cat "$FIX/c.txt" 2>/dev/null)"
assert "merge commits are on main" \
  sh -c "git -C '$FIX' log --oneline main | grep -q 'merge workstream 1'"
assert_eq "pre-merge ref recorded at run-start commit" "$BASE0" \
  "$(git -C "$FIX" rev-parse refs/codex-implement/feat-x/pre-merge 2>/dev/null)"

# push happened: session branch delivered
assert_eq "origin main == local main" \
  "$(git -C "$FIX" rev-parse main)" "$(git -C "$ORIGIN" rev-parse main)"

# retry / attempt counts via stub
count() { wc -l < "$STUB_DIR/calls-$1" 2>/dev/null | tr -d ' ' || echo 0; }
assert_eq "workstream 1 invoked once" 1 "$(count WS-OK)"
assert_eq "workstream 2 invoked twice (retry with failure context)" 2 "$(count WS-FAILONCE)"
assert_eq "workstream 3 (hang) invoked retries+1 times" 2 "$(count WS-HANG)"
assert_eq "workstream 4 (noop) invoked retries+1 times" 2 "$(count WS-NOOP)"
assert_eq "merge conflict resolved via codex exactly once" 1 "$(count MERGE-RESOLVE)"

if [ "$TEST_IMPLEMENT_RUNNER" = "daemon" ]; then
  assert "default runner is daemon" grep -q '^codex:implement: runner=daemon$' "$SCRATCH/run.log"
  assert "daemon runner handles workstream attempts" \
    grep -q '^WS-OK .* name=feat-x/w1 a1$' "$STUB_DIR/invocations.log"
  assert "daemon workstream retry has attempt name" \
    grep -q '^WS-FAILONCE .* name=feat-x/w2 a2$' "$STUB_DIR/invocations.log"
  assert "daemon runner handles merge conflicts" \
    grep -q '^MERGE-RESOLVE .* name=feat-x/merge-6$' "$STUB_DIR/invocations.log"
  assert_eq "codex stub handles no task attempts under daemon" 0 \
    "$(grep -E '^(WS-|MERGE-RESOLVE)' "$STUB_DIR/invocations.log" | grep -vc ' name=' || true)"
else
  assert "explicit exec runner is reported" grep -q '^codex:implement: runner=exec$' "$SCRATCH/run.log"
fi

# progress output
assert_eq "first attempts omit attempt number" 0 "$(grep -c 'attempt 1' "$SCRATCH/run.log" || true)"
assert "retry start appends attempt number" \
  grep -q '\[workstream 2\] starting (attempt 2)' "$SCRATCH/run.log"
assert "retry pass includes attempt number" grep -q '\[workstream 2\] PASS (attempt 2)' "$SCRATCH/run.log"
assert "check failure uses concise FAIL output" grep -q '\[workstream 2\] FAIL (exit 1)' "$SCRATCH/run.log"
assert "summary includes passed and failed workstreams" \
  grep -q '^codex:implement: PASS \[1 2 5 6\] FAIL \[3 4\]$' "$SCRATCH/run.log"
assert "successful merge uses uppercase status" \
  grep -q '^codex:implement: \[merge\] workstream 1 MERGED$' "$SCRATCH/run.log"
assert "merge uses concise PASS output" \
  grep -q '^codex:implement: \[merge\] PASS$' "$SCRATCH/run.log"
assert "PR output is concise" \
  grep -q '^codex:implement: PR is https://github.com/example/app/pull/42$' "$SCRATCH/run.log"
assert "console summary uses compact fields" \
  grep -q '^codex:implement: summary: feature=feat-x base=main workstreams=6 pass=4 fail=2 merged=4 post-merge=pass pushed$' "$SCRATCH/run.log"
assert "push output omits the branch name" \
  grep -q '^codex:implement: pushed to origin$' "$SCRATCH/run.log"
assert "delivery target announced" \
  grep -q "^codex:implement: delivering onto branch 'main' in $FIX$" "$SCRATCH/run.log"
assert "delivery target precedes logs" \
  awk '/^codex:implement: delivering onto/{d=NR} /^codex:implement: logs:/{l=NR} END {exit !(d && l && d < l)}' "$SCRATCH/run.log"

# worktree pool cleaned up entirely (no feature worktree exists at all)
assert "worktree root fully removed" test ! -e "$WT_ROOT"
assert_eq "git worktree list has no workstream worktrees" 0 \
  "$(git -C "$FIX" worktree list | grep -c "$WT_ROOT" || true)"
L="$(git -C "$FIX" rev-parse --absolute-git-dir)/codex-implement/feat-x/logs"
assert "pool respected concurrency=2 (no w3 created)" test ! -e "$L/w3.create.log"

# merged + clean-failed workstream branches deleted (failed workstreams had no commits)
assert_eq "no workstream branches left" 0 "$(git -C "$FIX" branch --list 'workstreams/feat-x/*' | wc -l | tr -d ' ')"

# status files
ST="$(git -C "$FIX" rev-parse --absolute-git-dir)/codex-implement/feat-x/status"
assert "summary.json exists" test -f "$ST/summary.json"
assert_eq "4 workstreams passed" 4 "$(grep -c '"result": "pass"' "$ST"/workstream-*.json | awk -F: '{s+=$2} END {print s}')"
assert_eq "2 workstreams failed" 2 "$(grep -c '"result": "fail"' "$ST"/workstream-*.json | awk -F: '{s+=$2} END {print s}')"
assert "summary records delivery to main" grep -q '"delivered_to": "main"' "$ST/summary.json"
assert "summary records no restore" grep -q '"restored": false' "$ST/summary.json"

# failed workstreams not on the session branch
assert "no stray files from failed workstreams" test ! -e "$FIX/hang.txt"

# implement.sh never reviews; review.sh is the caller's step after the push
assert "implement.sh does not invoke the review" test ! -e "$STUB_DIR/calls-REVIEW"
assert "summary carries no review field" test "$(grep -c '"review' "$ST/summary.json")" = 0
assert_eq "no GitHub review is requested" 0 "$(grep -c 'GitHub review' "$SCRATCH/run.log" || true)"
assert "no PR comment is posted" test ! -e "$STUB_DIR/gh-comment"

# ---------- scenario 2: all green, existing PR for the session branch ----------
# WS-C1 rewrites c.txt (scenario 1 left "ok-merged"), so the workstream has a real diff
printf 'WS-C1 write c.txt (variant 1)\n' > "$FIX/workstreams2.txt"
git -C "$FIX" add workstreams2.txt && git -C "$FIX" commit -qm "workstreams2" && git -C "$FIX" push -q origin main
set +e
GH_VIEW_OK=1 "$IMPLEMENT" --workstreams "$FIX/workstreams2.txt" --base main --feature feat-y \
  --check "sh ./check.sh" --concurrency 1 --retries 0 --timeout 3 \
  --repo "$FIX" > "$SCRATCH/run2.log" 2>&1
RC2=$?
set -e 2>/dev/null || true
echo "---- scenario 2 exit=$RC2 (log: $SCRATCH/run2.log) ----"

assert_eq "all-green run exits 0" 0 "$RC2"
assert "origin main advanced with the new merge" \
  sh -c "git -C '$ORIGIN' log --oneline main | grep -q 'merge workstream 1'"
assert_eq "origin main == local main after delivery" \
  "$(git -C "$FIX" rev-parse main)" "$(git -C "$ORIGIN" rev-parse main)"
assert "existing PR path updates instead of creating" \
  grep -q '^codex:implement: existing PR updates$' "$SCRATCH/run2.log"
assert "feat-y worktree root fully removed" test ! -e "$(dirname "$FIX")/.codex-implement-feat-y"
assert "all-green summary only includes passed workstreams" \
  grep -q '^codex:implement: PASS \[1\]$' "$SCRATCH/run2.log"
assert "all-green console summary omits zero values" \
  grep -q '^codex:implement: summary: feature=feat-y base=main workstreams=1 pass=1 merged=1 post-merge=pass pushed$' "$SCRATCH/run2.log"

# ---------- scenario 3: red post-merge check restores the session branch ----------
printf 'WS-D write d.txt\nWS-E write e.txt\n' > "$FIX/workstreams3.txt"
git -C "$FIX" add workstreams3.txt && git -C "$FIX" commit -qm "workstreams3" && git -C "$FIX" push -q origin main
PRE3="$(git -C "$FIX" rev-parse main)"
ORIGIN3="$(git -C "$ORIGIN" rev-parse main)"
set +e
"$IMPLEMENT" --workstreams "$FIX/workstreams3.txt" --feature feat-z \
  --check "sh ./check.sh" --concurrency 2 --retries 0 --timeout 3 \
  --repo "$FIX" > "$SCRATCH/run3.log" 2>&1
RC3=$?
set -e 2>/dev/null || true
echo "---- scenario 3 exit=$RC3 (log: $SCRATCH/run3.log) ----"

assert_eq "red post-merge exits 1" 1 "$RC3"
assert_eq "session branch restored to pre-merge commit" "$PRE3" "$(git -C "$FIX" rev-parse main)"
assert "restored worktree has no d.txt" test ! -e "$FIX/d.txt"
assert "restored worktree has no e.txt" test ! -e "$FIX/e.txt"
assert_eq "both workstream branches kept for autopsy" 2 \
  "$(git -C "$FIX" branch --list 'workstreams/feat-z/*' | wc -l | tr -d ' ')"
assert "restore is reported" \
  grep -q 'session branch restored' "$SCRATCH/run3.log"
assert "restored summary token present" \
  sh -c "grep -q 'post-merge=fail restored' '$SCRATCH/run3.log'"
assert_eq "origin main untouched by red run" "$ORIGIN3" "$(git -C "$ORIGIN" rev-parse main)"
ST3="$(git -C "$FIX" rev-parse --absolute-git-dir)/codex-implement/feat-z/status"
assert "summary records the restore" grep -q '"restored": true' "$ST3/summary.json"
git -C "$FIX" branch -q -D workstreams/feat-z/1 workstreams/feat-z/2

# ---------- scenario 4: guard rails ----------
echo dirt >> "$FIX/spec.md"
set +e
"$IMPLEMENT" --workstreams "$FIX/workstreams2.txt" --feature feat-w \
  --check "sh ./check.sh" --repo "$FIX" > "$SCRATCH/run4.log" 2>&1
RC4=$?
set -e 2>/dev/null || true
git -C "$FIX" checkout -q -- spec.md

assert "modified tracked file at start is a warning, not fatal" \
  sh -c "! grep -q 'FATAL' '$SCRATCH/run4.log'"
assert "dirty warning names the situation" \
  grep -q 'session worktree is dirty; workstreams branch from HEAD' "$SCRATCH/run4.log"
assert "dirty-start run still executed its workstream" \
  grep -q '\[workstream 1\] starting' "$SCRATCH/run4.log"
git -C "$FIX" branch -q -D workstreams/feat-w/1 2>/dev/null || true

set +e
"$IMPLEMENT" --workstreams "$FIX/workstreams2.txt" --base other --feature feat-v \
  --check "sh ./check.sh" --repo "$FIX" > "$SCRATCH/run5.log" 2>&1
RC5=$?
set -e 2>/dev/null || true

assert_eq "base/checkout mismatch is fatal" 1 "$RC5"
assert "mismatch guard names both branches" \
  sh -c "grep -q \"repo has 'main' checked out but --base is 'other'\" '$SCRATCH/run5.log'"

# ---------- scenario 5: tree dirty at merge time, clean again later -> delivery waits, then merges ----------
printf 'WS-DIRTY write f.txt while dirtying the session repo\n' > "$FIX/workstreams6.txt"
git -C "$FIX" add workstreams6.txt && git -C "$FIX" commit -qm "workstreams6" && git -C "$FIX" push -q origin main
set +e
STUB_DIRTY_REPO="$FIX" "$IMPLEMENT" --workstreams "$FIX/workstreams6.txt" --feature feat-u \
  --check "sh ./check.sh" --concurrency 1 --retries 0 --timeout 3 --deliver-wait 20 \
  --repo "$FIX" > "$SCRATCH/run6.log" 2>&1
RC6=$?
set -e 2>/dev/null || true
echo "---- scenario 5 exit=$RC6 (log: $SCRATCH/run6.log) ----"

assert_eq "delivery waits out transient dirt and exits 0" 0 "$RC6"
assert "waiting is reported" grep -q '\[merge\] waiting for a clean session worktree' "$SCRATCH/run6.log"
assert_eq "f.txt merged onto main after the wait" "ok-f" "$(cat "$FIX/f.txt" 2>/dev/null)"
assert "spec.md restored by the time delivery ran" git -C "$FIX" diff --quiet -- spec.md
assert "delivery lock released" test ! -e "$(git -C "$FIX" rev-parse --absolute-git-dir)/codex-implement/deliver.lock"

# ---------- scenario 6: dirt outlives --deliver-wait -> merge blocked, branch kept ----------
printf 'WS-DIRTY write f.txt while dirtying the session repo\n' > "$FIX/workstreams7.txt"
git -C "$FIX" rm -q f.txt   # scenario 5 landed f.txt; remove it so this workstream has a diff
git -C "$FIX" add workstreams7.txt && git -C "$FIX" commit -qm "workstreams7" && git -C "$FIX" push -q origin main
set +e
STUB_DIRTY_REPO="$FIX" STUB_DIRTY_SECS=12 "$IMPLEMENT" --workstreams "$FIX/workstreams7.txt" --feature feat-t \
  --check "sh ./check.sh" --concurrency 1 --retries 0 --timeout 3 --deliver-wait 1 \
  --repo "$FIX" > "$SCRATCH/run7.log" 2>&1
RC7=$?
set -e 2>/dev/null || true
git -C "$FIX" checkout -q -- spec.md
echo "---- scenario 6 exit=$RC7 (log: $SCRATCH/run7.log) ----"

assert_eq "dirt outliving deliver-wait blocks the merge" 1 "$RC7"
assert "blocked message names the wait" grep -q 'session worktree stayed dirty' "$SCRATCH/run7.log"
assert_eq "blocked run keeps the workstream branch" 1 \
  "$(git -C "$FIX" branch --list 'workstreams/feat-t/*' | wc -l | tr -d ' ')"
git -C "$FIX" branch -q -D workstreams/feat-t/1

# ---------- scenario 7: lock released after the post-merge check; push survives a remote that moved ----------
printf 'WS-OK write a.txt per spec.md\n' > "$FIX/workstreams8.txt"
echo "stale" > "$FIX/a.txt"
# During the post-merge check (the one check that runs in the session repo, after the merge and
# before the push) a second clone lands an unrelated commit on origin, once.
cat > "$FIX/sibling.sh" <<'EOF'
#!/bin/sh
[ "$(pwd -P)" = "$(cd "$SIBLING_REPO" && pwd -P)" ] || exit 0
[ -e "$SIBLING_CLONE/.landed" ] && exit 0
echo "sibling" > "$SIBLING_CLONE/sibling.txt"
git -C "$SIBLING_CLONE" add sibling.txt
git -C "$SIBLING_CLONE" commit -qm "sibling commit during the post-merge check"
git -C "$SIBLING_CLONE" push -q origin HEAD:main
touch "$SIBLING_CLONE/.landed"
EOF
git -C "$FIX" add workstreams8.txt a.txt sibling.sh && git -C "$FIX" commit -qm "workstreams8" && git -C "$FIX" push -q origin main
CLONE="$SCRATCH/sibling-clone"
git clone -q -b main "$ORIGIN" "$CLONE" && git -C "$CLONE" config user.email t@t && git -C "$CLONE" config user.name t
: > "$STUB_DIR/invocations.log"
set +e
SIBLING_CLONE="$CLONE" SIBLING_REPO="$FIX" "$IMPLEMENT" --workstreams "$FIX/workstreams8.txt" --feature feat-s \
  --check "sh ./check.sh && sh ./sibling.sh" --concurrency 1 --retries 0 --timeout 3 \
  --repo "$FIX" > "$SCRATCH/run8.log" 2>&1
RC8=$?
set -e 2>/dev/null || true
echo "---- scenario 7 exit=$RC8 (log: $SCRATCH/run8.log) ----"

assert_eq "run with a moved remote still delivers (exit 0)" 0 "$RC8"
assert "push retry is reported" grep -q 'push rejected (remote moved)' "$SCRATCH/run8.log"
assert "push eventually succeeded" grep -q '^codex:implement: pushed to origin$' "$SCRATCH/run8.log"
assert_eq "origin main == local main after retry" \
  "$(git -C "$FIX" rev-parse main)" "$(git -C "$ORIGIN" rev-parse main)"
assert "sibling commit is on the delivered branch" test -f "$FIX/sibling.txt"
assert_eq "workstream content is on the delivered branch" "ok-ws1" "$(cat "$FIX/a.txt")"
assert "re-check ran on the combined branch" test -f "$(git -C "$FIX" rev-parse --absolute-git-dir)/codex-implement/feat-s/logs/push-recheck.log"

# ---------- scenario 8: a commit rejected by the pre-commit hook is a failure, not a silent pass ----------
# Regression: the workstream commit ran without checking its exit code. A repo pre-commit hook that
# rejects the commit (e.g. a failing format:check) left the branch with zero commits, so the merge
# was a no-op — yet the run reported the workstream PASS and merged, and the edits silently vanished.
printf 'WS-HOOKREJECT write hookbad.txt that the pre-commit hook rejects\n' > "$FIX/workstreams9.txt"
git -C "$FIX" add workstreams9.txt && git -C "$FIX" commit -qm "workstreams9" && git -C "$FIX" push -q origin main
HOOK="$(git -C "$FIX" rev-parse --absolute-git-dir)/hooks/pre-commit"
cat > "$HOOK" <<'EOF'
#!/bin/sh
# Reject any commit that stages hookbad.txt (stand-in for a failing format:check).
if git diff --cached --name-only | grep -q '^hookbad.txt$'; then
  echo "pre-commit: hookbad.txt is not allowed" >&2
  exit 1
fi
exit 0
EOF
chmod +x "$HOOK"
PRE9="$(git -C "$ORIGIN" rev-parse main)"
set +e
"$IMPLEMENT" --workstreams "$FIX/workstreams9.txt" --feature feat-r \
  --check "sh ./check.sh" --concurrency 1 --retries 1 --timeout 3 \
  --repo "$FIX" > "$SCRATCH/run9.log" 2>&1
RC9=$?
set -e 2>/dev/null || true
rm -f "$HOOK"
echo "---- scenario 8 exit=$RC9 (log: $SCRATCH/run9.log) ----"

assert "hook-rejected commit is reported, not swallowed" \
  grep -q '\[workstream 1\].*commit rejected' "$SCRATCH/run9.log"
assert "hook-rejected workstream is never reported PASS" \
  sh -c "! grep -q '\[workstream 1\] PASS' '$SCRATCH/run9.log'"
ST9="$(git -C "$FIX" rev-parse --absolute-git-dir)/codex-implement/feat-r/status"
assert "hook-rejected workstream recorded as fail" grep -q '"result": "fail"' "$ST9/workstream-1.json"
assert "hookbad.txt never landed on the session branch" test ! -e "$FIX/hookbad.txt"
assert_eq "origin main untouched by the hook-rejected run" "$PRE9" "$(git -C "$ORIGIN" rev-parse main)"
git -C "$FIX" branch -q -D workstreams/feat-r/1 2>/dev/null || true

# ---------- scenario 9: a passed workstream whose branch is empty at merge is surfaced, not silently "merged" ----------
# Regression: a daemon-runner collision could land a workstream's commit off its own branch, so the branch
# reached the merge at tip==base. `git merge --no-ff` then reports "already up to date" (exit 0) and the run
# counted it MERGED while the edits silently vanished. The ahead-of-base guard must catch this.
printf 'WS-STRAY writes stray.txt but its commit lands off the workstream branch\n' > "$FIX/workstreams10.txt"
git -C "$FIX" add workstreams10.txt && git -C "$FIX" commit -qm "workstreams10" && git -C "$FIX" push -q origin main
PRE10="$(git -C "$ORIGIN" rev-parse main)"
set +e
"$IMPLEMENT" --workstreams "$FIX/workstreams10.txt" --feature feat-q \
  --check "sh ./check.sh" --concurrency 1 --retries 0 --timeout 3 \
  --repo "$FIX" > "$SCRATCH/run10.log" 2>&1
RC10=$?
set -e 2>/dev/null || true
echo "---- scenario 9 exit=$RC10 (log: $SCRATCH/run10.log) ----"

assert_eq "empty passed-branch makes the run partial (exit 2)" 2 "$RC10"
assert "empty branch is never reported MERGED" \
  sh -c "! grep -q '\[merge\] workstream 1 MERGED' '$SCRATCH/run10.log'"
assert "empty branch is surfaced as EMPTY" \
  grep -q '\[merge\] workstream 1 EMPTY' "$SCRATCH/run10.log"
assert "stray.txt never landed on the session branch" test ! -e "$FIX/stray.txt"
ST10="$(git -C "$FIX" rev-parse --absolute-git-dir)/codex-implement/feat-q/status"
assert "summary records zero merged" grep -q '"merged": 0' "$ST10/summary.json"
assert "summary records one merge_failed" grep -q '"merge_failed": 1' "$ST10/summary.json"
assert_eq "empty-branch workstream branch kept for inspection" 1 \
  "$(git -C "$FIX" branch --list 'workstreams/feat-q/*' | wc -l | tr -d ' ')"
assert_eq "origin main untouched by the empty-branch run" "$PRE10" "$(git -C "$ORIGIN" rev-parse main)"
git -C "$FIX" branch -q -D workstreams/feat-q/1 2>/dev/null || true

# ---------- scenario 11: an untracked file in the session worktree never delays delivery ----------
printf 'WS-UNTRACKED write f.txt and drop an untracked file in the session repo\n' > "$FIX/workstreams11.txt"
git -C "$FIX" add workstreams11.txt && git -C "$FIX" commit -qm "workstreams11" && git -C "$FIX" push -q origin main
echo "launch notes" > "$FIX/untracked-notes.txt"
set +e
STUB_DIRTY_REPO="$FIX" "$IMPLEMENT" --workstreams "$FIX/workstreams11.txt" --feature feat-p \
  --check "sh ./check.sh" --concurrency 1 --retries 0 --timeout 3 --deliver-wait 1 \
  --repo "$FIX" > "$SCRATCH/run11.log" 2>&1
RC11=$?
set -e 2>/dev/null || true
rm -f "$FIX/late.txt" "$FIX/untracked-notes.txt"
echo "---- scenario 11 exit=$RC11 (log: $SCRATCH/run11.log) ----"

assert_eq "untracked files do not block delivery" 0 "$RC11"
assert "no clean-tree wait for untracked files" \
  sh -c "! grep -q 'waiting for a clean session worktree' '$SCRATCH/run11.log'"
assert "untracked files do not trigger the dirty-start warning" \
  sh -c "! grep -q 'session worktree is dirty' '$SCRATCH/run11.log'"
assert_eq "f.txt merged despite the untracked file" "ok-f" "$(cat "$FIX/f.txt" 2>/dev/null)"

# ---------- scenario 12: a multi-line brief in the workstreams file is rejected before any run ----------
cat > "$FIX/workstreams12.txt" <<'EOF2'
# review-fix brief

implement workstream "review-fix" per spec.md, applying these fixes:
1. first fix
2. second fix
- a bullet
EOF2
set +e
"$IMPLEMENT" --workstreams "$FIX/workstreams12.txt" --feature feat-o \
  --check "sh ./check.sh" --repo "$FIX" > "$SCRATCH/run12.log" 2>&1
RC12=$?
set -e 2>/dev/null || true
rm -f "$FIX/workstreams12.txt"

assert_eq "list lines in the workstreams file are fatal" 1 "$RC12"
assert "fatal names the offending line as numbered in the file" grep -q 'FATAL.*line 4.*brief' "$SCRATCH/run12.log"
assert "no workstream launched from a fragmented brief" \
  sh -c "! grep -q '\[workstream 1\] starting' '$SCRATCH/run12.log'"

# ---------- scenario 13: a workstream file colliding with an untracked file fails the merge without a resolver ----------
printf 'WS-OK write a.txt per spec.md\n' > "$FIX/workstreams13.txt"
git -C "$FIX" rm -q a.txt && git -C "$FIX" add workstreams13.txt \
  && git -C "$FIX" commit -qm "workstreams13; a.txt no longer tracked" && git -C "$FIX" push -q origin main
echo "my scratch" > "$FIX/a.txt"
rm -f "$STUB_DIR/calls-MERGE-RESOLVE"
set +e
"$IMPLEMENT" --workstreams "$FIX/workstreams13.txt" --feature feat-n \
  --check "sh ./check.sh" --concurrency 1 --retries 0 --timeout 3 --deliver-wait 1 \
  --repo "$FIX" > "$SCRATCH/run13.log" 2>&1
RC13=$?
set -e 2>/dev/null || true
echo "---- scenario 13 exit=$RC13 (log: $SCRATCH/run13.log) ----"

assert_eq "untracked collision fails the merge" 2 "$RC13"
assert "untracked collision is named, not called a conflict" \
  grep -q '\[merge\] workstream 1 BLOCKED by untracked file(s): a.txt' "$SCRATCH/run13.log"
assert "no codex resolver launched for an untracked collision" test ! -e "$STUB_DIR/calls-MERGE-RESOLVE"
assert_eq "untracked file left untouched" "my scratch" "$(cat "$FIX/a.txt")"
assert_eq "collision workstream branch kept" 1 \
  "$(git -C "$FIX" branch --list 'workstreams/feat-n/1' | wc -l | tr -d ' ')"
rm -f "$FIX/a.txt"
git -C "$FIX" branch -q -D workstreams/feat-n/1 2>/dev/null || true

echo
if [ "$FAILS" -eq 0 ]; then echo "ALL TESTS PASSED"; else echo "$FAILS TEST(S) FAILED"; tail -40 "$SCRATCH/run.log"; echo "-- run3 --"; tail -30 "$SCRATCH/run3.log"; echo "-- run9 --"; tail -30 "$SCRATCH/run9.log"; exit 1; fi
