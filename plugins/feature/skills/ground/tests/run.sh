#!/bin/sh
# ABOUTME: Checks check-paths.sh, check-overlap.sh and check-anchors.py output and exit codes against small fixture docs.
# ABOUTME: Usage: run from a git repo root (chadwallet): <plugin>/skills/ground/tests/run.sh
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
check="$here/../check-paths.sh"
fail=0
expect() { # name expected-output expected-exit actual-output actual-exit
  if [ "$4" = "$2" ] && [ "$5" = "$3" ]; then echo "PASS: $1"; else
    echo "FAIL: $1"; echo "  expected exit $3:"; printf '%s\n' "$2" | sed 's/^/    /'
    echo "  got exit $5:"; printf '%s\n' "$4" | sed 's/^/    /'; fail=1; fi
}
git rev-parse --show-toplevel >/dev/null 2>&1 || { echo "run from inside a git repo" >&2; exit 2; }

out=$("$check" "$here/fixture-plan.md"); rc=$?
expect "missing path reported, exit 1" "MISSING: website/src/no-such-file-anywhere.ts" 1 "$out" $rc

out=$("$check" "$here/fixture-plan.md" 'no-such-file'); rc=$?
expect "skip regex silences the miss" "" 0 "$out" $rc

out=$("$check" "$here/fixture-clean.md"); rc=$?
expect "clean doc prints nothing, exit 0" "" 0 "$out" $rc

out=$("$check" "$here/fixture-ambiguous.md"); rc=$?
case "$out" in "AMBIGUOUS: index.ts ("[0-9]*" matches)") a=ok ;; *) a="$out" ;; esac
expect "bare basename with many matches is flagged" ok 1 "$a" $rc

out=$("$check" "$here/fixture-ext.md"); rc=$?
expect "toml paths are checked" "MISSING: proxy/no-such-config.toml" 1 "$out" $rc

out=$(cd / && "$check" "$here/fixture-clean.md" 2>&1); rc=$?
expect "refuses to run outside a git repo" "check-paths.sh: not inside a git repo" 2 "$out" $rc

out=$(cd "$(git rev-parse --show-toplevel)/scripts" && "$check" "$here/fixture-clean.md"); rc=$?
expect "resolves from the repo root when run in a subdirectory" "" 0 "$out" $rc

out=$("$check" "$here/fixture-range.md"); rc=$?
expect "paths with line ranges are checked" "MISSING: website/src/no-such-ranged-file.ts" 1 "$out" $rc

out=$("$check" "$here/fixture-brackets.md"); rc=$?
expect "bracketed route paths are checked" "MISSING: mobile/app/token/[nope].tsx" 1 "$out" $rc

overlap="$here/../check-overlap.sh"

out=$("$overlap" "$here/fixture-overlap.md"); rc=$?
expect "a file on two workstreams' Files: lines is flagged" \
  "OVERLAP: mobile/shared/data/api/core/client.ts (home-lanes, token-tabs)" 1 "$out" $rc

out=$("$overlap" "$here/fixture-overlap-clean.md"); rc=$?
expect "disjoint workstreams pass; Files: outside a workstream section is ignored" "" 0 "$out" $rc

anchors="$here/../check-anchors.py"
flags() { printf '%s\n' "$1" | grep -v '^    ' ; }

out=$("$anchors" "$here/fixture-anchors-clean.md"); rc=$?
expect "anchors whose sentence symbol sits near the cited lines pass" "" 0 "$out" $rc

out=$("$anchors" "$here/fixture-anchors-wrong-file.md"); rc=$?
expect "symbol absent near the cited lines is flagged" \
  "UNVERIFIED: mobile/shared/data/api/core/client.ts:211-214 — none of [TokenInfo] within 3 lines" 1 "$(flags "$out")" $rc

out=$("$anchors" "$here/fixture-anchors-nosymbol.md"); rc=$?
expect "anchor with no symbol or quote in its sentence is flagged" \
  "NOSYMBOL: proxy/test/tokenListSources.test.ts:264-268 — the sentence names nothing to look for" 1 "$(flags "$out")" $rc

out=$("$anchors" "$here/fixture-anchors-unanchored.md"); rc=$?
expect "function claim without a line anchor is flagged" \
  "UNANCHORED: launchpadLabel() — no path:line anchor in its paragraph cites it" 1 "$(flags "$out")" $rc

out=$("$anchors" "$here/fixture-anchors-eof.md"); rc=$?
expect "past-EOF line and file absent at base are flagged" \
  "PAST-EOF: website/src/components/chadwallet/token/launchpad.ts:900 (file has 36 lines at 321ab7087)
MISSING: website/src/no-such-file-anywhere.ts (at 321ab7087)" 1 "$(flags "$out")" $rc

out=$("$anchors" --at 321ab7087 "$here/fixture-anchors-nobase.md"); rc=$?
expect "--at pins the commit when the doc has no Base line" "" 0 "$out" $rc

out=$("$anchors" "$here/fixture-anchors-skip.md"); rc=$?
expect "non-repo anchor is flagged without the skip regex" "MISSING: recordings/session.mp4 (at 321ab7087)" 1 "$(flags "$out")" $rc

out=$("$anchors" "$here/fixture-anchors-skip.md" 'recordings/'); rc=$?
expect "skip regex silences the non-repo anchor" "" 0 "$out" $rc

out=$(cd / && "$anchors" "$here/fixture-anchors-clean.md" 2>&1); rc=$?
expect "anchors: refuses to run outside a git repo" "check-anchors.py: not inside a git repo" 2 "$out" $rc

exit $fail
