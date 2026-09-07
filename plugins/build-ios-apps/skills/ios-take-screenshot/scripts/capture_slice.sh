#!/usr/bin/env bash
# Capture one full-resolution slice from a simulator this run holds into a PNG.
# Refuses "booted", which simctl resolves to an arbitrary simulator when
# several are up, and refuses a simulator claimed by another run or by none.
set -euo pipefail

usage() {
  echo 'Usage: capture_slice.sh --simulator <udid> --run <RUN_ID> --out <slice.png>' >&2
  exit 2
}

UDID=""; RUN=""; OUT=""
while [ $# -gt 0 ]; do
  case "$1" in
    --simulator) UDID="${2:-}"; shift 2 ;;
    --run)       RUN="${2:-}"; shift 2 ;;
    --out)       OUT="${2:-}"; shift 2 ;;
    -h|--help)   usage ;;
    *) echo "unknown argument: $1" >&2; usage ;;
  esac
done
[ -n "$UDID" ] && [ -n "$RUN" ] && [ -n "$OUT" ] || usage
if [ "$UDID" = "booted" ]; then
  echo '"booted" names no simulator; pass the UDID' >&2
  exit 2
fi

LOCK="${TMPDIR:-/tmp}/ios-screenshot-lock.$UDID.json"
if [ ! -f "$LOCK" ]; then
  echo "simulator $UDID is not claimed; run claim_simulator.py first" >&2
  exit 3
fi
HOLDER="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("run",""))' "$LOCK")"
if [ "$HOLDER" != "$RUN" ]; then
  echo "simulator $UDID is held by run $HOLDER, not $RUN; wait for its release or abort" >&2
  exit 3
fi

if ! ERR="$(xcrun simctl io "$UDID" screenshot --type=png "$OUT" 2>&1 >/dev/null)"; then
  printf '%s\n' "$ERR" >&2
  exit 1
fi
echo "$OUT"
