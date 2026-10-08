#!/bin/bash
# ABOUTME: Prints the browse build id, a digest of GSTACK_COMMIT, patches/ and the build scripts; any change to them gives a new id.
# ABOUTME: build.sh names its default install dir and .version after it, and bin/browse finds the build by it.
set -euo pipefail

SKILL_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$SKILL_DIR"
find GSTACK_COMMIT patches scripts/build.sh scripts/apply-patches.sh scripts/*.swift -type f -print0 | LC_ALL=C sort -z | xargs -0 shasum -a 256 | shasum -a 256 | cut -c1-16
