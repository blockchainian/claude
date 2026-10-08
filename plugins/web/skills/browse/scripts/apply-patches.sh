#!/usr/bin/env bash
# ABOUTME: Applies patches/ in filename order to a clean gstack source worktree, staging each change.
# ABOUTME: `apply-patches.sh SOURCE_WORKTREE`; refuses a dirty source so existing changes are preserved.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
skill_dir="$(cd "$script_dir/.." && pwd)"
source_root="${1:?usage: apply-patches.sh SOURCE_WORKTREE}"

cd "$source_root"
[[ -f browse/src/cli.ts ]] || { echo "gstack source missing" >&2; exit 1; }
[[ -z "$(git status --porcelain --untracked-files=all)" ]] || {
    echo "Source must be clean; existing changes have been preserved" >&2
    exit 1
}
for patch in "$skill_dir"/patches/[0-9][0-9][0-9][0-9]-*.patch; do
    [[ -f "$patch" ]] || { echo "No numbered patches found" >&2; exit 1; }
    echo "Applying $(basename "$patch")"
    git apply --check --index "$patch"
    git apply --index "$patch"
done
