#!/bin/bash
# ABOUTME: Re-vendors browse from a gstack checkout at a given commit into vendor/gstack, keeping this skill's package.json and bun.lock.
# ABOUTME: Then checks every patch in patches/ still applies; regenerate bun.lock and update UPSTREAM.md by hand after it.
set -euo pipefail

if [ $# -ne 2 ]; then
  echo "usage: vendor.sh <gstack-checkout> <commit>" >&2
  exit 2
fi

GSTACK=$1
COMMIT=$2
SKILL_DIR=$(cd "$(dirname "$0")/.." && pwd -P)
VENDOR="$SKILL_DIR/vendor/gstack"

# The upstream paths browse needs; UPSTREAM.md explains what is left out and why.
PATHS=(
  browse/src
  browse/bin
  browse/scripts
  browse/test/test-server.ts
  browse/test/fixtures/basic.html
  lib/egress-receipt.ts
  lib/fs-atomic.ts
  patches/playwright-core@1.62.1.patch
  LICENSE
)

STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT

git -C "$GSTACK" archive "$COMMIT" "${PATHS[@]}" | tar -x -C "$STAGE"
cp "$VENDOR/package.json" "$VENDOR/bun.lock" "$STAGE/"

rm -rf "$VENDOR"
mkdir -p "$VENDOR"
cp -R "$STAGE/." "$VENDOR/"

CHECK=$(mktemp -d)
cp -R "$VENDOR/." "$CHECK/"
for patch_file in "$SKILL_DIR"/patches/*.patch; do
  (cd "$CHECK" && git apply "$patch_file") || { echo "patch no longer applies: $patch_file" >&2; rm -rf "$CHECK"; exit 1; }
done
rm -rf "$CHECK"

echo "vendored gstack $(git -C "$GSTACK" rev-parse --short "$COMMIT"); compare package.json with upstream's, regenerate bun.lock, update UPSTREAM.md"
