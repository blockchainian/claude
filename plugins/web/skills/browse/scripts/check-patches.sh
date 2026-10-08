#!/usr/bin/env bash
# ABOUTME: Checks that every patch applies in order to the gstack submodule's checked-out commit, in a fresh
# ABOUTME: worktree at .worktrees/check-patches; a conflict leaves that worktree for inspection.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
skill_dir="$(cd "$script_dir/.." && pwd)"
worktree="$skill_dir/.worktrees/check-patches"

[[ ! -e "$worktree" ]] || { echo "Inspect existing check worktree: $worktree" >&2; exit 1; }
cd "$skill_dir/gstack"
commit="$(git rev-parse HEAD)"
mkdir -p "$skill_dir/.worktrees"
git worktree add --detach "$worktree" "$commit"
"$script_dir/apply-patches.sh" "$worktree"
cd "$worktree"
git restore --source=HEAD --staged --worktree -- .
cd "$skill_dir/gstack"
git worktree remove "$worktree"
echo "All patches apply in order to $commit"
