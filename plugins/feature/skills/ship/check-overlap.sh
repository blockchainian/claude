#!/bin/sh
# ABOUTME: Flags files that two workstreams of a plan both list on their `Files:` lines; workstreams
# ABOUTME: run in parallel, so a shared file is a merge conflict. Usage: check-overlap.sh <plan.md>; exits 1 on overlap.
doc=$1
[ -n "$doc" ] && [ -f "$doc" ] || { echo "usage: check-overlap.sh <plan.md>" >&2; exit 2; }
awk '
  /^## /        { section = $0; ws = "" }
  /^### /       { ws = $0; sub(/^### *`?/, "", ws); sub(/`?[[:space:]]+—.*$/, "", ws); sub(/`.*$/, "", ws) }
  /^Files:/ && ws != "" && section ~ /^## (UX )?[Ww]orkstreams/ {
    line = $0
    while (match(line, /`[^`]+`/)) {
      path = substr(line, RSTART + 1, RLENGTH - 2)
      line = substr(line, RSTART + RLENGTH)
      if (path ~ /^[][A-Za-z0-9_.\/()-]+$/ && index(owners[path], "\t" ws "\t") == 0) {
        owners[path] = owners[path] "\t" ws "\t"
        n[path]++
      }
    }
  }
  END {
    status = 0
    for (path in n) if (n[path] > 1) {
      ids = owners[path]; gsub(/\t\t/, ", ", ids); gsub(/^\t|\t$/, "", ids)
      print "OVERLAP: " path " (" ids ")"; status = 1
    }
    exit status
  }
' "$doc"
