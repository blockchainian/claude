#!/bin/bash
# ABOUTME: Tests for workstream.sh — open, check, merge and base against a throwaway git repo,
# ABOUTME: covering the green path, a red post-merge check, a conflict and a dirty session tree.
set -u
cd "$(dirname "$0")" || exit 1
WS=$(cd .. && pwd)/workstream.sh

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
expect_eq() { # <name> <expected> <actual>
  if [ "$2" = "$3" ]; then report "$1" pass; else report "$1" fail "expected '$2' got '$3'"; fi
}

SANDBOX=$(cd "$(mktemp -d)" && pwd -P)
trap 'rm -rf "$SANDBOX"' EXIT
REPO=$SANDBOX/repo
mkdir -p "$REPO/app/node_modules/dep" "$REPO/apps/web/client/node_modules/deep"
echo "module.exports = 1" > "$REPO/app/node_modules/dep/index.js"
echo "module.exports = 2" > "$REPO/apps/web/client/node_modules/deep/index.js"
git -C "$REPO" init -q -b main
git -C "$REPO" config user.email test@example.com
git -C "$REPO" config user.name test
echo "node_modules" > "$REPO/.gitignore"
echo "one" > "$REPO/a.txt"
echo "shared" > "$REPO/shared.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm base
BASE_SHA=$(git -C "$REPO" rev-parse HEAD)
cd "$REPO" || exit 1

# --- open ---
out=$("$WS" open auth 2>&1); rc=$?
expect_eq "open prints the worktree path" "$SANDBOX/.workstream-auth" "$out"
expect_eq "open exits 0" 0 "$rc"
expect_eq "open branches from HEAD" "$BASE_SHA" "$(git -C "$SANDBOX/.workstream-auth" rev-parse HEAD)"
expect_eq "open checks out workstream/<id>" "workstream/auth" "$(git -C "$SANDBOX/.workstream-auth" rev-parse --abbrev-ref HEAD)"
[ -f "$SANDBOX/.workstream-auth/app/node_modules/dep/index.js" ] && report "open copies node_modules into the worktree" pass || report "open copies node_modules into the worktree" fail
[ -f "$SANDBOX/.workstream-auth/apps/web/client/node_modules/deep/index.js" ] && report "open copies node_modules at any depth" pass || report "open copies node_modules at any depth" fail
expect_eq "open records the base once" "$BASE_SHA" "$("$WS" base)"
out=$("$WS" open auth 2>&1); rc=$?
expect_eq "a second open of the same id is refused" 1 "$rc"
case "$out" in *"already exists"*) report "the refusal names the cause" pass ;; *) report "the refusal names the cause" fail "$out" ;; esac

# --- check ---
"$WS" check auth 'test "$(cat a.txt)" = one && touch ran-here' >/dev/null 2>&1; rc=$?
expect_eq "check runs the command in the worktree and passes its exit code" 0 "$rc"
[ -f "$SANDBOX/.workstream-auth/ran-here" ] && [ ! -f "$REPO/ran-here" ] && report "check runs in the worktree, not the session tree" pass || report "check runs in the worktree, not the session tree" fail
"$WS" check auth 'exit 7' >/dev/null 2>&1; rc=$?
expect_eq "check passes a red exit code through" 7 "$rc"
rm -f "$SANDBOX/.workstream-auth/ran-here"
"$WS" check auth 'touch first' 'exit 3' 'touch third' >/dev/null 2>&1; rc=$?
expect_eq "several commands run in order and stop at the first red" 3 "$rc"
[ -f "$SANDBOX/.workstream-auth/first" ] && [ ! -f "$SANDBOX/.workstream-auth/third" ] && report "the commands after a red one do not run" pass || report "the commands after a red one do not run" fail
rm -f "$SANDBOX/.workstream-auth/first"

# --- merge, green ---
echo "two" > "$SANDBOX/.workstream-auth/b.txt"
git -C "$SANDBOX/.workstream-auth" add b.txt && git -C "$SANDBOX/.workstream-auth" commit -qm "add b"
out=$("$WS" merge auth 'test -f b.txt' 'test -f a.txt' 2>&1); rc=$?
expect_eq "merge exits 0 when every post-merge check is green" 0 "$rc"
expect_eq "merge prints the recorded base" "$BASE_SHA" "$out"
[ -f "$REPO/b.txt" ] && report "merge lands the workstream's commit on the session branch" pass || report "merge lands the workstream's commit on the session branch" fail
expect_eq "merge is a merge commit, not a fast-forward" 2 "$(git -C "$REPO" rev-list --parents -n1 HEAD | wc -w | tr -d ' ' | awk '{print $1-1}')"
[ ! -d "$SANDBOX/.workstream-auth" ] && report "merge removes the worktree" pass || report "merge removes the worktree" fail
git -C "$REPO" rev-parse -q --verify workstream/auth >/dev/null 2>&1 && report "merge deletes the branch" fail || report "merge deletes the branch" pass
expect_eq "base survives a merge" "$BASE_SHA" "$("$WS" base)"

# --- merge, red check ---
HEAD_BEFORE=$(git -C "$REPO" rev-parse HEAD)
"$WS" open red >/dev/null 2>&1
echo "three" > "$SANDBOX/.workstream-red/c.txt"
git -C "$SANDBOX/.workstream-red" add c.txt && git -C "$SANDBOX/.workstream-red" commit -qm "add c"
out=$("$WS" merge red 'exit 1' 2>&1); rc=$?
expect_eq "merge exits 1 when the post-merge check is red" 1 "$rc"
expect_eq "a red check restores the session branch" "$HEAD_BEFORE" "$(git -C "$REPO" rev-parse HEAD)"
[ -d "$SANDBOX/.workstream-red" ] && report "a red check keeps the worktree for the fix" pass || report "a red check keeps the worktree for the fix" fail
case "$out" in *"check failed"*) report "a red check is named in the output" pass ;; *) report "a red check is named in the output" fail "$out" ;; esac
"$WS" merge red 'true' >/dev/null 2>&1
expect_eq "the same workstream merges once its check is green" 0 "$?"

# --- merge, conflict ---
HEAD_BEFORE=$(git -C "$REPO" rev-parse HEAD)
"$WS" open clash >/dev/null 2>&1
echo "theirs" > "$SANDBOX/.workstream-clash/shared.txt"
git -C "$SANDBOX/.workstream-clash" commit -qam "theirs"
echo "ours" > "$REPO/shared.txt"
git -C "$REPO" commit -qam "ours"
HEAD_OURS=$(git -C "$REPO" rev-parse HEAD)
out=$("$WS" merge clash 'true' 2>&1); rc=$?
expect_eq "a conflicting merge exits 1" 1 "$rc"
expect_eq "a conflicting merge is aborted" "$HEAD_OURS" "$(git -C "$REPO" rev-parse HEAD)"
expect_eq "the session tree is clean after the abort" "" "$(git -C "$REPO" status --porcelain)"
case "$out" in *"shared.txt"*) report "the conflicted file is named" pass ;; *) report "the conflicted file is named" fail "$out" ;; esac

# --- merge, dirty session tree ---
echo "dirty" >> "$REPO/a.txt"
out=$("$WS" merge clash 'true' 2>&1); rc=$?
expect_eq "merge refuses a dirty session tree" 1 "$rc"
case "$out" in *"uncommitted"*) report "the dirty refusal names the cause" pass ;; *) report "the dirty refusal names the cause" fail "$out" ;; esac
git -C "$REPO" checkout -q a.txt

# --- open, dependency copy fails ---
chmod 000 "$REPO/app/node_modules/dep/index.js"
out=$("$WS" open nodeps 2>&1); rc=$?
chmod 644 "$REPO/app/node_modules/dep/index.js"
expect_eq "open exits 1 when node_modules cannot be copied" 1 "$rc"
case "$out" in *"node_modules"*) report "the copy failure names node_modules" pass ;; *) report "the copy failure names node_modules" fail "$out" ;; esac
[ ! -d "$SANDBOX/.workstream-nodeps" ] && report "a failed open leaves no worktree behind" pass || report "a failed open leaves no worktree behind" fail
git -C "$REPO" rev-parse -q --verify workstream/nodeps >/dev/null 2>&1 && report "a failed open leaves no branch behind" fail || report "a failed open leaves no branch behind" pass

# --- base --clear, usage ---
"$WS" base --clear
expect_eq "base --clear forgets the recorded base" "$(git -C "$REPO" rev-parse HEAD)" "$("$WS" base)"
"$WS" >/dev/null 2>&1; rc=$?
expect_eq "no subcommand exits 2" 2 "$rc"
"$WS" check nope true >/dev/null 2>&1; rc=$?
expect_eq "check on an unopened workstream exits 1" 1 "$rc"
(cd / && "$WS" base >/dev/null 2>&1); rc=$?
expect_eq "outside a git repo exits 2" 2 "$rc"

printf '%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
