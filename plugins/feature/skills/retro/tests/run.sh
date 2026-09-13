#!/usr/bin/env bash
# ABOUTME: Runs extract.py against the hermetic fixture session and asserts the join + token math.
set -euo pipefail
cd "$(dirname "$0")/.."

out="$(python3 extract.py --root tests/fixture/projects --codex-root tests/fixture/codex fixture-sess)"
fail=0
expect() { if ! grep -qF "$1" <<<"$out"; then echo "FAIL: expected to find: $1"; fail=1; fi; }
reject() { if grep -qF "$1" <<<"$out"; then echo "FAIL: should not find: $1"; fail=1; fi; }

expect "orchestrator own cost: 130 tok (billable)"
expect "spawn ledger: {'general-purpose': 1, 'feature:ux-verifier': 1, 'codex:codex-rescue': 1}"
expect "subagent TOTAL: 1,300"
expect "claude-fable-5-1=1,000 (77%)"
expect "claude-sonnet-5=300 (23%)"       # defined agent: model from its own jsonl, not the null meta field
expect "WS-A parity fixes"               # label joined via toolUseId t1
expect "session grand total (orchestrator + subagents): 1,430"

# codex lane: deterministic thread-id join, billable = 1000+200(cache_write)+300+100 = 1600
expect "codex lane (1 rollouts, billable tokens; exact thread-id join)"
expect "[exact] codex-thread-1"
expect "codex TOTAL: 1,600"
reject "other-thread"                     # different cwd + not a recorded thread id → excluded

if [ "$fail" -eq 0 ]; then echo "retro/extract.py: all assertions passed"; else echo "$out"; exit 1; fi
