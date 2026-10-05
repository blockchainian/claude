#!/bin/sh
set -eu
scripts=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ "${1:-}" = "--check" ]; then
  command -v node >/dev/null && echo 'node: present' || echo 'node: missing'
  command -v npm >/dev/null && echo 'npm: present' || echo 'npm: missing'
  [ -d "$scripts/node_modules/camoufox-js" ] && echo 'dependencies: present' || echo 'dependencies: missing'
  if [ -d "$scripts/node_modules/camoufox-js" ]; then
    node "$scripts/camoufox-install.mjs" --check
  else echo 'camoufox: missing'; fi
  exit 0
fi
cd "$scripts"
npm install
node "$scripts/camoufox-install.mjs"
