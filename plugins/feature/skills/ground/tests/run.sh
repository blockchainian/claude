#!/bin/sh
# ABOUTME: Checks check-paths.sh, check-overlap.sh and check-anchors.py output and exit codes against small fixture docs.
# ABOUTME: Builds a throwaway git repo with known source files so the suite is hermetic; run from anywhere: <plugin>/skills/ground/tests/run.sh
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
check="$here/../check-paths.sh"
fail=0
expect() { # name expected-output expected-exit actual-output actual-exit
  if [ "$4" = "$2" ] && [ "$5" = "$3" ]; then echo "PASS: $1"; else
    echo "FAIL: $1"; echo "  expected exit $3:"; printf '%s\n' "$2" | sed 's/^/    /'
    echo "  got exit $5:"; printf '%s\n' "$4" | sed 's/^/    /'; fail=1; fi
}

# --- build a throwaway repo with known source files, so no fixture depends on
# --- this plugin repo's own files or any other project's paths or SHAs.
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
repo="$tmp/repo"
mkdir -p "$repo/src/routes" "$repo/src/a" "$repo/src/b" "$repo/config" "$repo/docs" "$repo/sub"
git -C "$repo" init -q
git -C "$repo" config user.email test@example.com
git -C "$repo" config user.name test

cat > "$repo/src/app.ts" <<'EOF'
// ABOUTME: sample source file for ground test fixtures
// ABOUTME: intentionally small and generic

export interface AppOptions {
  verbose: boolean
}

export function loadOptions(): AppOptions {
  return { verbose: false }
}

function helper(): void {
  return
}

export function noop(): void {
  helper()
}

function pad(n: number): string {
  return String(n)
}

export const CONFIG_MAP: Record<string, string> = { a: 'alpha', b: 'beta' }

export function parseConfig(key: string): string {
  const value = CONFIG_MAP[key]
  if (value) return value
  if (key.includes('/')) return key
  return key.toUpperCase()
}

export function unused(): number {
  return 0
}

export function unused2(): number {
  return 1
}
export const END_MARKER = true
EOF

cat > "$repo/src/other.ts" <<'EOF'
// ABOUTME: sample source file for ground test fixtures
// ABOUTME: security-ish record example

export interface SecurityFlags {
  renouncedMint: boolean
  frozenAuthority: boolean
  mutableMetadata: boolean
  isHoneypot: boolean
}

export function checkFlags(flags: SecurityFlags): boolean {
  if (flags.isHoneypot) return false
  if (flags.frozenAuthority) return true
  return true
}

export interface DisplayOptions {
  compact: boolean
}

export function formatDisplay(opts: DisplayOptions): string {
  return opts.compact ? 'compact' : 'full'
}

export const DEFAULT_OPTIONS: DisplayOptions = { compact: false }

export const VERSION = 1
EOF

cat > "$repo/src/rows.ts" <<'EOF'
// ABOUTME: sample source file for ground test fixtures
// ABOUTME: row normalization helpers

export interface Row {
  id: string
  value: number
}

function clamp(n: number): number {
  return n < 0 ? 0 : n
}
export function normalizeRow(row: Row): Row {
  return { id: row.id, value: clamp(row.value) }
}

export function mapRows(rows: Row[]): Row[] {
  return rows.map(normalizeRow)
}
EOF

cat > "$repo/src/apiClient.ts" <<'EOF'
// ABOUTME: sample source file for ground test fixtures
// ABOUTME: API client type example

export interface RequestOptions {
  timeout: number
}

export interface UserRecord {
  id: string
  email: string
  createdAt: string
}

export function fetchUser(id: string): UserRecord | null {
  return null
}
EOF

cat > "$repo/docs/constants.md" <<'EOF'
# Constants

This doc lists shared constants.

## Limits

- `FLOOR_LIMITS` bounds the minimum value accepted.
- `CEILING_LIMITS` bounds the maximum value accepted.
EOF

cat > "$repo/config/app.toml" <<'EOF'
[app]
name = "sample"
EOF

cat > "$repo/src/routes/[id].tsx" <<'EOF'
export default function Route() {
  return null
}
EOF

echo "export const A = 1" > "$repo/src/a/index.ts"
echo "export const B = 1" > "$repo/src/b/index.ts"

git -C "$repo" add -A
git -C "$repo" commit -q -m "fixture repo"
sha=$(git -C "$repo" rev-parse --short=9 HEAD)
applines=$(git -C "$repo" show "$sha:src/app.ts" | python3 -c "import sys; print(len(sys.stdin.read().rstrip(chr(10)).split(chr(10))))")

sed "s/@SHA@/$sha/" "$here/fixture-anchors-clean.md" > "$tmp/fixture-anchors-clean.md"

out=$(cd "$repo" && "$check" "$here/fixture-plan.md"); rc=$?
expect "missing path reported, exit 1" "MISSING: src/missing.ts" 1 "$out" $rc

out=$(cd "$repo" && "$check" "$here/fixture-plan.md" 'missing'); rc=$?
expect "skip regex silences the miss" "" 0 "$out" $rc

out=$(cd "$repo" && "$check" "$here/fixture-clean.md"); rc=$?
expect "clean doc prints nothing, exit 0" "" 0 "$out" $rc

out=$(cd "$repo" && "$check" "$here/fixture-ambiguous.md"); rc=$?
case "$out" in "AMBIGUOUS: index.ts ("[0-9]*" matches)") a=ok ;; *) a="$out" ;; esac
expect "bare basename with many matches is flagged" ok 1 "$a" $rc

out=$(cd "$repo" && "$check" "$here/fixture-ext.md"); rc=$?
expect "toml paths are checked" "MISSING: config/missing.toml" 1 "$out" $rc

out=$(cd / && "$check" "$here/fixture-clean.md" 2>&1); rc=$?
expect "refuses to run outside a git repo" "check-paths.sh: not inside a git repo" 2 "$out" $rc

out=$(cd "$repo/sub" && "$check" "$here/fixture-clean.md"); rc=$?
expect "resolves from the repo root when run in a subdirectory" "" 0 "$out" $rc

out=$(cd "$repo" && "$check" "$here/fixture-range.md"); rc=$?
expect "paths with line ranges are checked" "MISSING: src/missing-ranged.ts" 1 "$out" $rc

out=$(cd "$repo" && "$check" "$here/fixture-brackets.md"); rc=$?
expect "bracketed route paths are checked" "MISSING: src/routes/[missing].tsx" 1 "$out" $rc

overlap="$here/../check-overlap.sh"

out=$("$overlap" "$here/fixture-overlap.md"); rc=$?
expect "a file on two workstreams' Files: lines is flagged" \
  "OVERLAP: src/shared/client.ts (home-lanes, token-tabs)" 1 "$out" $rc

out=$("$overlap" "$here/fixture-overlap-clean.md"); rc=$?
expect "disjoint workstreams pass; Files: outside a workstream section is ignored" "" 0 "$out" $rc

anchors="$here/../check-anchors.py"
flags() { printf '%s\n' "$1" | grep -v '^    ' ; }

out=$(cd "$repo" && "$anchors" "$tmp/fixture-anchors-clean.md"); rc=$?
expect "anchors whose sentence symbol sits near the cited lines pass" "" 0 "$out" $rc

out=$(cd "$repo" && "$anchors" --at "$sha" "$here/fixture-anchors-wrong-file.md"); rc=$?
expect "symbol absent near the cited lines is flagged" \
  "UNVERIFIED: src/other.ts:20-24 — none of [UserRecord] within 3 lines" 1 "$(flags "$out")" $rc

out=$(cd "$repo" && "$anchors" --at "$sha" "$here/fixture-anchors-nosymbol.md"); rc=$?
expect "anchor with no symbol or quote in its sentence is flagged" \
  "NOSYMBOL: src/other.ts:12-14 — the sentence names nothing to look for" 1 "$(flags "$out")" $rc

out=$(cd "$repo" && "$anchors" --at "$sha" "$here/fixture-anchors-unanchored.md"); rc=$?
expect "function claim without a line anchor is flagged" \
  "UNANCHORED: parseConfig() — no path:line anchor in its paragraph cites it" 1 "$(flags "$out")" $rc

out=$(cd "$repo" && "$anchors" --at "$sha" "$here/fixture-anchors-eof.md"); rc=$?
expect "past-EOF line and file absent at base are flagged" \
  "PAST-EOF: src/app.ts:900 (file has $applines lines at $sha)
MISSING: src/no-such-file.ts (at $sha)" 1 "$(flags "$out")" $rc

out=$(cd "$repo" && "$anchors" --at "$sha" "$here/fixture-anchors-nobase.md"); rc=$?
expect "--at pins the commit when the doc has no Base line" "" 0 "$out" $rc

out=$(cd "$repo" && "$anchors" --at "$sha" "$here/fixture-anchors-skip.md"); rc=$?
expect "non-repo anchor is flagged without the skip regex" "MISSING: assets/demo.mp4 (at $sha)" 1 "$(flags "$out")" $rc

out=$(cd "$repo" && "$anchors" --at "$sha" "$here/fixture-anchors-skip.md" 'assets/'); rc=$?
expect "skip regex silences the non-repo anchor" "" 0 "$out" $rc

out=$(cd / && "$anchors" "$here/fixture-anchors-nobase.md" 2>&1); rc=$?
expect "anchors: refuses to run outside a git repo" "check-anchors.py: not inside a git repo" 2 "$out" $rc

acceptance="$here/../check-acceptance.py"

out=$("$acceptance" "$here/fixture-acceptance-problem-clean.md"); rc=$?
expect "one-arg: well-formed acceptance criteria pass" "" 0 "$out" $rc

out=$("$acceptance" "$here/fixture-acceptance-problem-none.md"); rc=$?
expect "one-arg: an empty acceptance section is flagged" "NO-ACCEPTANCE: no acceptance criteria found" 1 "$out" $rc

out=$("$acceptance" "$here/fixture-acceptance-problem-dup.md"); rc=$?
expect "one-arg: a duplicate criterion id is flagged" "DUPLICATE: AC1" 1 "$out" $rc

out=$("$acceptance" "$here/fixture-acceptance-plan-clean.md" "$here/fixture-acceptance-problem-clean.md"); rc=$?
expect "two-arg: every criterion referenced by the plan passes" "" 0 "$out" $rc

out=$("$acceptance" "$here/fixture-acceptance-plan-uncovered.md" "$here/fixture-acceptance-problem-clean.md"); rc=$?
expect "two-arg: a criterion no test references is UNCOVERED" "UNCOVERED: AC2" 1 "$out" $rc

out=$("$acceptance" "$here/fixture-acceptance-plan-unknown.md" "$here/fixture-acceptance-problem-clean.md"); rc=$?
expect "two-arg: a plan reference to a nonexistent criterion is UNKNOWN" "UNKNOWN: AC3" 1 "$out" $rc

exit $fail
