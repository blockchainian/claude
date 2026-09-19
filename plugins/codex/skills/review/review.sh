#!/usr/bin/env bash
# ABOUTME: Reviews the commits a repository gained between two SHAs with a read-only codex exec and
# ABOUTME: writes the findings as JSON ({findings: [{file, line, severity, claim}]}) for triage.
set -u

usage() {
  cat >&2 <<'EOT'
Usage: review.sh REPO BASE HEAD OUT_JSON [SPEC]

  REPO      repository checkout to review in
  BASE      commit the changes start after (excluded)
  HEAD      commit the changes end at (included)
  OUT_JSON  file that receives the findings JSON; a .log file beside it gets codex's output
  SPEC      optional repo-relative spec or plan the changes are judged against

Runs a plain `codex exec` in a read-only sandbox with review instructions and review-schema.json,
not `codex exec review --base`, which refuses custom instructions. Severity is must-fix or nit.
Env: IMPLEMENT_CODEX overrides the codex binary (default: codex); REVIEW_TIMEOUT seconds (default 2400).
EOT
  exit 1
}

[ $# -ge 4 ] || usage
REPO="$1" BASE="$2" HEAD_SHA="$3" OUT="$4" SPEC="${5:-}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CODEX="${IMPLEMENT_CODEX:-codex}"
TIMEOUT_S="${REVIEW_TIMEOUT:-2400}"
LOG="${OUT%.json}.log"
note() { echo "codex:review: $*" >&2; }

PROMPT="Review the changes this repository gained between commit $BASE and commit $HEAD_SHA (git diff $BASE $HEAD_SHA)${SPEC:+, against the intent in $SPEC}. Every finding you return costs a verification pass and a fix round, so return only findings you are confident are real and must be fixed before this ships, each labelled must-fix; return no nits, and no findings at all when nothing qualifies. A finding qualifies only when all of these hold: (1) the change breaks behaviour, violates an invariant the spec or AGENTS.md states, leaves an input or error path unhandled, opens a security hole, or slows something a user would notice; (2) this change introduced it — do not report bugs that were already there; (3) it rests on the code as written, not on assumptions about the codebase or the author's intent, and a change the spec asks for is never a finding; (4) when the claim is that the change breaks other code, name the file and line it breaks — that it might is not a finding. Do not report style, naming, refactoring, comments or docs. One finding per distinct issue. In each claim state the input or scenario that triggers the failure and what goes wrong. Cite the file and line of every finding, inside the diff."

note "reviewing $BASE..$HEAD_SHA in $REPO"
if timeout "$TIMEOUT_S" "$CODEX" exec -C "$REPO" -s read-only -c model_reasoning_effort=high \
     --output-schema "$SCRIPT_DIR/review-schema.json" -o "$OUT" "$PROMPT" > "$LOG" 2>&1; then
  note "result: $OUT"
else
  note "codex review failed (see $LOG)"
  exit 1
fi
