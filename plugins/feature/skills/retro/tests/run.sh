#!/usr/bin/env bash
# ABOUTME: Runs extract.py against the hermetic fixture session and asserts the join + token math.
set -euo pipefail
cd "$(dirname "$0")/.."

out="$(python3 extract.py --root tests/fixture/projects --codex-root tests/fixture/codex fixture-sess)"
fail=0
expect() { if ! grep -qF "$1" <<<"$out"; then echo "FAIL: expected to find: $1"; fail=1; fi; }
reject() { if grep -qF "$1" <<<"$out"; then echo "FAIL: should not find: $1"; fail=1; fi; }

expect "orchestrator own cost: 150 tok (billable)"   # m-dup, split across two streamed lines, counted once (130 + one 20)

# context health: peak from cache_read, compaction deduped to second precision, image reads + re-reads
expect "peak 5,000 tok"
expect "1 auto-compaction(s) at 2026-09-13T00:05:00"
expect "2 image read(s) into main loop"
expect "2× shot.png"
expect "spawn ledger: {'general-purpose': 1, 'feature:ux-verifier': 1, 'codex:codex-rescue': 1}"
expect "subagent TOTAL: 1,300"
expect "claude-fable-5-1=1,000 (77%)"
expect "claude-sonnet-5=300 (23%)"       # defined agent: model from its own jsonl, not the null meta field
expect "WS-A parity fixes"               # label joined via toolUseId t1
expect "session grand total (orchestrator + subagents): 1,450"

# codex lane: deterministic thread-id join, billable = 1000+200(cache_write)+300+100 = 1600
expect "codex lane (1 rollouts, billable tokens; exact thread-id join)"
expect "[exact] codex-thread-1"
expect "codex TOTAL: 1,600"
expect "mean joined rollout: 1,600 tok"   # proxy multiplier for un-joinable failed workstreams
reject "other-thread"                     # different cwd + not a recorded thread id → excluded

# ---- efficacy.py: recurrence analysis over retro.json x fixes.jsonl ----
eout="$(python3 efficacy.py --root tests/fixture-efficacy)"
eexpect() { if ! grep -qF "$1" <<<"$eout"; then echo "FAIL (efficacy): expected: $1"; fail=1; fi; }
eexpect "retros: 3  fixes: 2"
eexpect "NO RECURRENCE — strong"                 # mechanical gate: waste structurally blocked
eexpect "NOT EFFECTIVE — recurs at 20.0% → 20.0%"   # judgment fix: waste came back
eexpect "mechanical-gate: 1/1"
eexpect "judgment: 0/1"

if [ "$fail" -eq 0 ]; then echo "retro/extract.py + efficacy.py: all assertions passed"; else echo "$out"; echo "$eout"; exit 1; fi
