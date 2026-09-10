#!/bin/sh
# ABOUTME: Flags file paths named in a plan or grounding doc that do not exist in the repo.
# ABOUTME: Usage: check-paths.sh <doc> [skip-regex]   — run anywhere inside the repo; exits 1 on any miss.
#
# Only backticked paths with a code or config extension are checked. A line containing
# "(new)" is skipped whole, so list files the doc creates one per line. A bare basename
# that matches exactly one file passes; more than one is reported as AMBIGUOUS.
doc=$1
skip=${2:-}
[ -n "$doc" ] || { echo "usage: check-paths.sh <doc> [skip-regex]" >&2; exit 2; }
case "$doc" in /*) ;; *) doc="$PWD/$doc" ;; esac
root=$(git rev-parse --show-toplevel 2>/dev/null) || { echo "check-paths.sh: not inside a git repo" >&2; exit 2; }
cd "$root" || exit 2
status=0
for p in $(grep -vE '\(new\)' "$doc" \
  | grep -ohE '`[][A-Za-z0-9_./()-]+\.(ts|tsx|js|mjs|sql|json|jsonc|sh|toml|ya?ml)(:[0-9]+(-[0-9]+)?)?`' \
  | tr -d '`' | cut -d: -f1 | sort -u); do
  [ -n "$skip" ] && printf '%s\n' "$p" | grep -qE "$skip" && continue
  [ -e "$p" ] && continue
  case "$p" in */*) echo "MISSING: $p"; status=1; continue ;; esac
  n=$(find . \( -name .git -o -name node_modules \) -prune -o -name "$p" -print | wc -l | tr -d ' ')
  case "$n" in
    0) echo "MISSING: $p"; status=1 ;;
    1) ;;
    *) echo "AMBIGUOUS: $p ($n matches)"; status=1 ;;
  esac
done
exit $status
