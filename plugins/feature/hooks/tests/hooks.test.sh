#!/bin/bash
# ABOUTME: Runs every hook case in cases.jsonl against the plugin's hook scripts and reports
# ABOUTME: pass/fail counts; exits 1 on any failure.
set -u
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
hooks=${HOOKS_DIR:-$(dirname -- "$here")}
cases=$here/cases.jsonl

pass=0
fail=0

while IFS= read -r line; do
  [ -n "$line" ] || continue
  hook=$(jq -r '.hook' <<<"$line")
  name=$(jq -r '.name' <<<"$line")
  expect=$(jq -r '.expect' <<<"$line")
  out=$(jq -c '.input' <<<"$line" | "$hooks/$hook" 2>&1)
  ok=false
  case "$expect" in
    allow)
      [ "$(jq -r '.hookSpecificOutput.permissionDecision // ""' <<<"$out" 2>/dev/null)" = "allow" ] && ok=true
      ;;
    pass)
      [ -z "$out" ] && ok=true
      ;;
    context:*)
      want=${expect#context:}
      ctx=$(jq -r '.hookSpecificOutput.additionalContext // ""' <<<"$out" 2>/dev/null)
      case "$ctx" in *"$want"*) ok=true ;; esac
      ;;
    input:*)
      want=${expect#input:}
      upd=$(jq -c '.hookSpecificOutput.updatedInput // ""' <<<"$out" 2>/dev/null)
      case "$upd" in *"$want"*) ok=true ;; esac
      ;;
    deny:*)
      want=${expect#deny:}
      dec=$(jq -r '.hookSpecificOutput.permissionDecision // ""' <<<"$out" 2>/dev/null)
      reason=$(jq -r '.hookSpecificOutput.permissionDecisionReason // ""' <<<"$out" 2>/dev/null)
      [ "$dec" = deny ] && case "$reason" in *"$want"*) ok=true ;; esac
      ;;
  esac
  if [ "$ok" = true ]; then
    pass=$((pass + 1))
  else
    fail=$((fail + 1))
    echo "FAIL [$hook] $name: expected $expect, got: ${out:-<no output>}"
  fi
done < "$cases"

echo "cases: $pass passed, $fail failed"
[ "$fail" = 0 ] || exit 1
