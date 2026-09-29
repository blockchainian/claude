#!/bin/bash
# ABOUTME: Runs one workstream's worktree lifecycle for the ship skill: open a worktree, run its
# ABOUTME: check there, merge it onto the session branch behind the same check, and report the base.
set -u

usage="usage: workstream.sh open <id> | check <id> <cmd> | merge <id> <cmd> | base [--clear]

  open  <id>        add ../.workstream-<id> on branch workstream/<id> from HEAD, copy node_modules,
                    record the base (HEAD before the first open) and print the worktree path
  check <id> <cmd>  run <cmd> in the workstream's worktree and exit with its status
  merge <id> <cmd>  merge workstream/<id> onto the session branch (--no-ff), run <cmd> in the
                    session tree, restore the branch if it is red, remove the worktree and branch
                    if it is green, and print the base
  base [--clear]    print the recorded base (HEAD when none is recorded); --clear forgets it"

die() { printf 'workstream.sh: %s\n' "$*" >&2; exit "${2:-1}"; }

COMMAND=${1:-}
[ -n "$COMMAND" ] || { echo "$usage" >&2; exit 2; }
ROOT=$(git rev-parse --show-toplevel 2>/dev/null) || die "not inside a git repo" 2
GIT_DIR=$(git -C "$ROOT" rev-parse --git-common-dir)
case "$GIT_DIR" in /*) ;; *) GIT_DIR=$ROOT/$GIT_DIR ;; esac
BASE_FILE=$GIT_DIR/workstream-base

worktree_of() { printf '%s/.workstream-%s' "$(dirname "$ROOT")" "$1"; }
branch_of() { printf 'workstream/%s' "$1"; }

open() {
  local id=$1 dir branch
  [ -n "$id" ] || die "open needs a workstream id" 2
  dir=$(worktree_of "$id"); branch=$(branch_of "$id")
  [ ! -e "$dir" ] || die "$dir already exists"
  ! git -C "$ROOT" rev-parse -q --verify "refs/heads/$branch" >/dev/null || die "branch $branch already exists"
  [ -f "$BASE_FILE" ] || git -C "$ROOT" rev-parse HEAD > "$BASE_FILE"
  git -C "$ROOT" worktree add -q -b "$branch" "$dir" HEAD || die "worktree add failed"
  # A fresh worktree carries only tracked files; the checks need each module's dependencies.
  find "$ROOT" -maxdepth 3 -name node_modules -type d -prune 2>/dev/null | while read -r modules; do
    local relative=${modules#"$ROOT"/}
    mkdir -p "$(dirname "$dir/$relative")"
    cp -Rc "$modules" "$dir/$relative" 2>/dev/null || cp -R "$modules" "$dir/$relative"
  done
  printf '%s\n' "$dir"
}

check() {
  local id=$1 cmd=${2:-} dir
  [ -n "$id" ] && [ -n "$cmd" ] || die "check needs a workstream id and a command" 2
  dir=$(worktree_of "$id")
  [ -d "$dir" ] || die "no worktree at $dir; open the workstream first"
  (cd "$dir" && bash -c "$cmd")
}

merge() {
  local id=$1 cmd=${2:-} dir branch conflicted
  [ -n "$id" ] && [ -n "$cmd" ] || die "merge needs a workstream id and a command" 2
  dir=$(worktree_of "$id"); branch=$(branch_of "$id")
  git -C "$ROOT" rev-parse -q --verify "refs/heads/$branch" >/dev/null || die "no branch $branch; open the workstream first"
  [ -z "$(git -C "$ROOT" status --porcelain --untracked-files=no)" ] || die "the session tree has uncommitted changes; commit or stash them before merging"
  if ! git -C "$ROOT" merge -q --no-ff -m "Merge workstream $id" "$branch" >/dev/null 2>&1; then
    conflicted=$(git -C "$ROOT" diff --name-only --diff-filter=U | tr '\n' ' ')
    git -C "$ROOT" merge --abort >/dev/null 2>&1
    die "merge of $branch conflicts in: ${conflicted:-unknown}; resolve in the worktree ($dir) and merge again"
  fi
  if ! (cd "$ROOT" && bash -c "$cmd"); then
    git -C "$ROOT" reset -q --hard ORIG_HEAD
    die "check failed after merging $branch; the session branch is restored and the worktree kept at $dir"
  fi
  [ -d "$dir" ] && git -C "$ROOT" worktree remove --force "$dir" >/dev/null 2>&1
  git -C "$ROOT" branch -q -D "$branch" >/dev/null 2>&1
  base
}

base() {
  if [ "${1:-}" = "--clear" ]; then rm -f "$BASE_FILE"; return 0; fi
  if [ -f "$BASE_FILE" ]; then cat "$BASE_FILE"; else git -C "$ROOT" rev-parse HEAD; fi
}

case "$COMMAND" in
  open) open "${2:-}" ;;
  check) check "${2:-}" "${3:-}" ;;
  merge) merge "${2:-}" "${3:-}" ;;
  base) base "${2:-}" ;;
  *) echo "$usage" >&2; exit 2 ;;
esac
