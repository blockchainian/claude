#!/bin/bash
# ABOUTME: Builds the patched browse CLI: copies vendor/gstack into an install dir, applies patches/ in order, installs and compiles.
# ABOUTME: Only --link repoints ~/.local/bin/browse at the build, which makes the running daemon restart on its next command.
set -euo pipefail

usage() {
  echo "usage: build.sh [--dest <dir>] [--link]" >&2
  echo "  --dest  install dir (default ~/.local/share/web-browse/<plugin version>)" >&2
  echo "  --link  point ~/.local/bin/browse at the built binary" >&2
  exit 2
}

SKILL_DIR=$(cd "$(dirname "$0")/.." && pwd -P)
PLUGIN_JSON="$SKILL_DIR/../../.claude-plugin/plugin.json"
PLUGIN_VERSION=$(sed -n 's/^  "version": "\([^"]*\)".*/\1/p' "$PLUGIN_JSON")
DEST="$HOME/.local/share/web-browse/$PLUGIN_VERSION"
LINK=0

while [ $# -gt 0 ]; do
  case "$1" in
    --dest) [ $# -ge 2 ] || usage; DEST=$2; shift 2 ;;
    --link) LINK=1; shift ;;
    *) usage ;;
  esac
done

command -v bun >/dev/null || { echo "bun not found on PATH" >&2; exit 1; }

# Build beside the destination and swap it in at the end, so a failed build never leaves a half-written install.
STAGE="$DEST.partial"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp -R "$SKILL_DIR/vendor/gstack/." "$STAGE/"

for patch_file in "$SKILL_DIR"/patches/*.patch; do
  (cd "$STAGE" && git apply "$patch_file")
done

cd "$STAGE"
bun install --frozen-lockfile
bun build --compile browse/src/cli.ts --outfile browse/dist/browse
bun build --compile browse/src/find-browse.ts --outfile browse/dist/find-browse
bash browse/scripts/build-node-server.sh

# The CLI restarts a daemon whose recorded version differs, so the version is a digest of everything that shapes the build.
BUILD_DIGEST=$(cd "$SKILL_DIR" && find vendor/gstack patches -type f | LC_ALL=C sort | xargs shasum -a 256 | shasum -a 256 | cut -c1-16)
printf '%s\n' "$PLUGIN_VERSION-$BUILD_DIGEST" > browse/dist/.version
chmod +x browse/dist/browse browse/dist/find-browse
rm -f .*.bun-build

cd "$SKILL_DIR"
rm -rf "$DEST"
mv "$STAGE" "$DEST"

if [ "$LINK" -eq 1 ]; then
  mkdir -p "$HOME/.local/bin"
  ln -sfn "$DEST/browse/dist/browse" "$HOME/.local/bin/browse"
  echo "linked $HOME/.local/bin/browse -> $DEST/browse/dist/browse"
fi

echo "$DEST/browse/dist/browse"
