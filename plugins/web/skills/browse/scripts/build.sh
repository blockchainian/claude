#!/bin/bash
# ABOUTME: Builds the patched browse CLI: copies vendor/gstack into an install dir, applies patches/ in order, installs and compiles.
# ABOUTME: The default install dir is <plugin data>/browse/<build id>, where the plugin's bin/browse launcher looks for it.
set -euo pipefail

usage() {
  echo "usage: build.sh [--dest <dir>]" >&2
  echo "  --dest  install dir (default \${CLAUDE_PLUGIN_DATA:-~/.claude/plugins/data/web-blockchainian}/browse/<build id>)" >&2
  exit 2
}

SKILL_DIR=$(cd "$(dirname "$0")/.." && pwd -P)
BUILD_ID=$(bash "$SKILL_DIR/scripts/build-id.sh")
DEST="${CLAUDE_PLUGIN_DATA:-$HOME/.claude/plugins/data/web-blockchainian}/browse/$BUILD_ID"

while [ $# -gt 0 ]; do
  case "$1" in
    --dest) [ $# -ge 2 ] || usage; DEST=$2; shift 2 ;;
    *) usage ;;
  esac
done

command -v bun >/dev/null || { echo "bun not found on PATH" >&2; exit 1; }

# Build beside the destination and swap it in at the end, so a failed build never leaves a half-written install.
STAGE="$DEST.partial"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp -R "$SKILL_DIR/vendor/gstack/." "$STAGE/"

# Stop git's repository discovery at the stage dir: inside a work tree (a dotfiles repo holding ~/.claude),
# `git apply` resolves paths from that repo's root and silently skips every patch.
STAGE_PARENT=$(cd "$(dirname "$STAGE")" && pwd -P)
for patch_file in "$SKILL_DIR"/patches/*.patch; do
  (cd "$STAGE" && GIT_CEILING_DIRECTORIES="$STAGE_PARENT" git apply "$patch_file")
done

cd "$STAGE"
bun install --frozen-lockfile
bun build --compile browse/src/cli.ts --outfile browse/dist/browse
bun build --compile browse/src/find-browse.ts --outfile browse/dist/find-browse
bash browse/scripts/build-node-server.sh

# The CLI restarts a daemon whose recorded version differs, so a new build id restarts the daemon once.
printf '%s\n' "$BUILD_ID" > browse/dist/.version
chmod +x browse/dist/browse browse/dist/find-browse
rm -f .*.bun-build

cd "$SKILL_DIR"
rm -rf "$DEST"
mv "$STAGE" "$DEST"

echo "$DEST/browse/dist/browse"
