#!/usr/bin/env bash
# Resolve an installed iOS app's bundle identifier on a device or a simulator.
# devicectl hides App Store apps unless --include-all-apps is passed, and does
# not see simulators at all; simulators are listed by simctl instead.
set -euo pipefail

usage() {
  cat >&2 <<'USAGE'
Usage: find_ios_app.sh (--device <udid> | --simulator <udid>) [--name <substring>] [--all]

  --device      Device UDID (from `xcrun devicectl list devices`, hardwareProperties.udid)
  --simulator   Simulator UDID (from `xcrun simctl list devices`), or "booted"
  --name        Case-insensitive substring matched against app name and bundle id
  --all         List every installed app instead of filtering

Prints JSON: [{"name": ..., "bundleId": ..., "version": ...}, ...]
USAGE
  exit 2
}

DEVICE=""; SIMULATOR=""; NAME=""; ALL=0
while [ $# -gt 0 ]; do
  case "$1" in
    --device)    DEVICE="${2:-}"; shift 2 ;;
    --simulator) SIMULATOR="${2:-}"; shift 2 ;;
    --name)      NAME="${2:-}"; shift 2 ;;
    --all)       ALL=1; shift ;;
    -h|--help) usage ;;
    *) echo "unknown argument: $1" >&2; usage ;;
  esac
done
[ -n "$DEVICE" ] || [ -n "$SIMULATOR" ] || usage
[ -z "$DEVICE" ] || [ -z "$SIMULATOR" ] || { echo "pass --device or --simulator, not both" >&2; usage; }
[ -n "$NAME" ] || [ "$ALL" -eq 1 ] || usage

OUT="$(mktemp "${TMPDIR:-/tmp}/ios-apps.XXXXXX.json")"
trap 'rm -f "$OUT"' EXIT

if [ -n "$SIMULATOR" ]; then
  # simctl prints an old-style plist, so convert it rather than parsing it.
  xcrun simctl listapps "$SIMULATOR" | plutil -convert json -o "$OUT" -- -
else
  xcrun devicectl device info apps \
    --device "$DEVICE" \
    --include-all-apps \
    --json-output "$OUT" >/dev/null
fi

NAME="$NAME" ALL="$ALL" python3 - "$OUT" <<'PY'
import json, os, sys

data = json.load(open(sys.argv[1]))
if isinstance(data, dict) and "result" in data:
    apps = [                                     # devicectl
        {"name": a.get("name") or "",
         "bundleId": a.get("bundleIdentifier") or "",
         "version": a.get("version")}
        for a in data.get("result", {}).get("apps", [])
    ]
else:
    apps = [                                     # simctl, keyed by bundle id
        {"name": a.get("CFBundleDisplayName") or a.get("CFBundleName") or "",
         "bundleId": bundle,
         "version": a.get("CFBundleShortVersionString") or a.get("CFBundleVersion")}
        for bundle, a in data.items()
    ]

needle = os.environ.get("NAME", "").lower()
show_all = os.environ.get("ALL") == "1"

rows = [a for a in apps if show_all or needle in f"{a['name']} {a['bundleId']}".lower()]
rows.sort(key=lambda r: r["name"].lower())
print(json.dumps(rows, indent=2))
if not rows:
    print(f"no app matched {needle!r} among {len(apps)} installed apps", file=sys.stderr)
    sys.exit(1)
PY
