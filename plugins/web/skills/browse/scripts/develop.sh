#!/usr/bin/env bash
# ABOUTME: Creates a development worktree of the gstack submodule at its committed pointer, with every patch
# ABOUTME: committed as the baseline; `develop.sh [WORKTREE]`, default .worktrees/develop. Export new work against HEAD.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
skill_dir="$(cd "$script_dir/.." && pwd)"
worktree="${1:-$skill_dir/.worktrees/develop}"
[[ "$worktree" = /* ]] || worktree="$PWD/$worktree"

[[ ! -e "$worktree" ]] || { echo "Worktree already exists: $worktree" >&2; exit 1; }
cd "$skill_dir"
commit="$(git rev-parse HEAD:./gstack)"
mkdir -p "$(dirname "$worktree")"
cd "$skill_dir/gstack"
git worktree add --detach "$worktree" "$commit"
"$script_dir/apply-patches.sh" "$worktree"
cd "$worktree"
git -c user.name='browse patch baseline' -c user.email='patch-baseline@localhost' \
    commit -m 'Apply personal browse patch baseline'
echo "Develop and test in: $worktree"
echo "Export only your new changes with: git diff --binary --cached HEAD"
