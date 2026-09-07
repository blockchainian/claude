#!/usr/bin/env bash
# Capture one full-resolution slice from a claimed simulator into a PNG.
# Refuses "booted", which simctl resolves to an arbitrary simulator when
# several are up, and refuses a simulator no run has claimed.
set -euo pipefail

usage() {
  echo 'Usage: capture_slice.sh --simulator <udid> --out <slice.png>' >&2
  exit 2
}

UDID=""; OUT=""
while [ $# -gt 0 ]; do
  case "$1" in
    --simulator) UDID="${2:-}"; shift 2 ;;
    --out)       OUT="${2:-}"; shift 2 ;;
    -h|--help)   usage ;;
    *) echo "unknown argument: $1" >&2; usage ;;
  esac
done
[ -n "$UDID" ] && [ -n "$OUT" ] || usage
if [ "$UDID" = "booted" ]; then
  echo '"booted" names no simulator; pass the UDID' >&2
  exit 2
fi

LOCK="${TMPDIR:-/tmp}/ios-screenshot-lock.$UDID.json"
if [ ! -f "$LOCK" ]; then
  echo "simulator $UDID is not claimed; run claim_simulator.py first" >&2
  exit 3
fi

if ! ERR="$(xcrun simctl io "$UDID" screenshot --type=png "$OUT" 2>&1 >/dev/null)"; then
  printf '%s\n' "$ERR" >&2
  exit 1
fi
echo "$OUT"
