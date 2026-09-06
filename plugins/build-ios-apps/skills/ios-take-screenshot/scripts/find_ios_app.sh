#!/usr/bin/env bash
# Resolve an installed iOS app's bundle identifier on a connected device.
# devicectl hides App Store apps unless --include-all-apps is passed, so a plain
# `devicectl device info apps` makes ordinary apps look uninstalled.
set -euo pipefail

usage() {
  cat >&2 <<'USAGE'
Usage: find_ios_app.sh --device <udid> [--name <substring>] [--all]

  --device   Device UDID (from `xcrun devicectl list devices`, hardwareProperties.udid)
  --name     Case-insensitive substring matched against app name and bundle id
  --all      List every installed app instead of filtering

Prints JSON: [{"name": ..., "bundleId": ..., "version": ...}, ...]
USAGE
  exit 2
}

DEVICE=""; NAME=""; ALL=0
while [ $# -gt 0 ]; do
  case "$1" in
    --device) DEVICE="${2:-}"; shift 2 ;;
    --name)   NAME="${2:-}"; shift 2 ;;
    --all)    ALL=1; shift ;;
    -h|--help) usage ;;
    *) echo "unknown argument: $1" >&2; usage ;;
  esac
done
[ -n "$DEVICE" ] || usage
[ -n "$NAME" ] || [ "$ALL" -eq 1 ] || usage

OUT="$(mktemp "${TMPDIR:-/tmp}/ios-apps.XXXXXX.json")"
trap 'rm -f "$OUT"' EXIT

xcrun devicectl device info apps \
  --device "$DEVICE" \
  --include-all-apps \
  --json-output "$OUT" >/dev/null

NAME="$NAME" ALL="$ALL" python3 - "$OUT" <<'PY'
import json, os, sys

data = json.load(open(sys.argv[1]))
apps = data.get("result", {}).get("apps", [])
needle = os.environ.get("NAME", "").lower()
show_all = os.environ.get("ALL") == "1"

rows = []
for a in apps:
    name = a.get("name") or ""
    bundle = a.get("bundleIdentifier") or ""
    if show_all or needle in f"{name} {bundle}".lower():
        rows.append({"name": name, "bundleId": bundle, "version": a.get("version")})

rows.sort(key=lambda r: r["name"].lower())
print(json.dumps(rows, indent=2))
if not rows:
    print(f"no app matched {needle!r} among {len(apps)} installed apps", file=sys.stderr)
    sys.exit(1)
PY
