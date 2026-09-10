#!/usr/bin/env bash
# ABOUTME: Shared scenario implementations for the execute engine's fake task runners.
# ABOUTME: Simulates workstream edits, retries, timeouts, and merge-conflict resolution.

scenario_log() { echo "$1 dir=$DIR${NAME:+ name=$NAME}" >> "${STUB_DIR:?}/invocations.log"; }
scenario_bump() { echo x >> "${STUB_DIR:?}/calls-$1"; }

run_stub_scenario() {
  case "$PROMPT" in
    *"merge conflict"*)
      scenario_bump MERGE-RESOLVE; scenario_log MERGE-RESOLVE
      echo "ok-merged" > "$DIR/c.txt"
      git -C "$DIR" add c.txt
      ;;
    *WS-OK*)
      scenario_bump WS-OK; scenario_log WS-OK
      echo "ok-ws1" > "$DIR/a.txt"
      ;;
    *WS-FAILONCE*)
      scenario_bump WS-FAILONCE; scenario_log WS-FAILONCE
      case "$PROMPT" in
        *"PREVIOUS ATTEMPT FAILED"*) echo "ok-ws2" > "$DIR/b.txt" ;;
        *) echo "bad" > "$DIR/b.txt" ;;
      esac
      ;;
    *WS-HANG*)
      scenario_bump WS-HANG; scenario_log WS-HANG
      if [ "${STUB_DAEMON_MODE:-0}" = "1" ]; then return 124; fi
      sleep 30
      ;;
    *WS-NOOP*)
      scenario_bump WS-NOOP; scenario_log WS-NOOP
      ;;
    *WS-C1*)
      scenario_bump WS-C1; scenario_log WS-C1
      echo "ok-ws5" > "$DIR/c.txt"
      ;;
    *WS-C2*)
      scenario_bump WS-C2; scenario_log WS-C2
      echo "ok-ws6" > "$DIR/c.txt"
      ;;
    *WS-DIRTY*)
      scenario_bump WS-DIRTY; scenario_log WS-DIRTY
      echo "ok-f" > "$DIR/f.txt"
      echo dirt > "${STUB_DIRTY_REPO:?}/late.txt"
      (sleep "${STUB_DIRTY_SECS:-3}"; rm -f "${STUB_DIRTY_REPO}/late.txt") >/dev/null 2>&1 &
      disown 2>/dev/null || true
      ;;
    *WS-D*)
      scenario_bump WS-D; scenario_log WS-D
      echo "ok-d" > "$DIR/d.txt"
      ;;
    *WS-E*)
      scenario_bump WS-E; scenario_log WS-E
      echo "ok-e" > "$DIR/e.txt"
      ;;
    *)
      scenario_bump UNKNOWN; scenario_log "UNKNOWN prompt=$PROMPT"
      return 1
      ;;
  esac
}
