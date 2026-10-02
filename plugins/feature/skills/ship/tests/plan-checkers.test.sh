#!/bin/sh
# ABOUTME: Checks check-paths.sh and check-overlap.sh output and exit codes against small fixture docs.
# ABOUTME: Builds a throwaway git repo with known source files so the suite is hermetic; run from anywhere: <plugin>/skills/ship/tests/plan-checkers.test.sh
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
check="$here/../scripts/check-paths.sh"
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
// ABOUTME: sample source file for plan checker test fixtures
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
// ABOUTME: sample source file for plan checker test fixtures
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
// ABOUTME: sample source file for plan checker test fixtures
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
// ABOUTME: sample source file for plan checker test fixtures
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
out=$(cd "$repo" && "$check" "$here/fixture/check-paths/missing-path.md"); rc=$?
expect "missing path reported, exit 1" "MISSING: src/missing.ts" 1 "$out" $rc

out=$(cd "$repo" && "$check" "$here/fixture/check-paths/missing-path.md" 'missing'); rc=$?
expect "skip regex silences the miss" "" 0 "$out" $rc

out=$(cd "$repo" && "$check" "$here/fixture/check-paths/all-paths-resolve.md"); rc=$?
expect "clean doc prints nothing, exit 0" "" 0 "$out" $rc

out=$(cd "$repo" && "$check" "$here/fixture/check-paths/ambiguous-basename.md"); rc=$?
case "$out" in "AMBIGUOUS: index.ts ("[0-9]*" matches)") a=ok ;; *) a="$out" ;; esac
expect "bare basename with many matches is flagged" ok 1 "$a" $rc

out=$(cd "$repo" && "$check" "$here/fixture/check-paths/config-extension.md"); rc=$?
expect "toml paths are checked" "MISSING: config/missing.toml" 1 "$out" $rc

out=$(cd / && "$check" "$here/fixture/check-paths/all-paths-resolve.md" 2>&1); rc=$?
expect "refuses to run outside a git repo" "check-paths.sh: not inside a git repo" 2 "$out" $rc

out=$(cd "$repo/sub" && "$check" "$here/fixture/check-paths/all-paths-resolve.md"); rc=$?
expect "resolves from the repo root when run in a subdirectory" "" 0 "$out" $rc

out=$(cd "$repo" && "$check" "$here/fixture/check-paths/line-range.md"); rc=$?
expect "paths with line ranges are checked" "MISSING: src/missing-ranged.ts" 1 "$out" $rc

out=$(cd "$repo" && "$check" "$here/fixture/check-paths/bracketed-route.md"); rc=$?
expect "bracketed route paths are checked" "MISSING: src/routes/[missing].tsx" 1 "$out" $rc

out=$(cd "$repo" && "$check" "$here/fixture/check-paths/marked-new.md"); rc=$?
expect "a path marked (new) once is skipped everywhere, ./ prefix included" "" 0 "$out" $rc

overlap="$here/../scripts/check-overlap.sh"

out=$("$overlap" "$here/fixture/check-overlap/shared-file.md"); rc=$?
expect "a file on two workstreams' Files: lines is flagged" \
  "OVERLAP: src/shared/client.ts (home-lanes, token-tabs)" 1 "$out" $rc

out=$("$overlap" "$here/fixture/check-overlap/disjoint.md"); rc=$?
expect "disjoint workstreams pass; Files: outside a workstream section is ignored" "" 0 "$out" $rc

exit $fail
