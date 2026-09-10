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

PROMPT="Review the changes this repository gained between commit $BASE and commit $HEAD_SHA (git diff $BASE $HEAD_SHA)${SPEC:+, against the intent in $SPEC}. Report a finding only when the change breaks behaviour, violates a stated invariant, or leaves an input or error path unhandled; label it must-fix. Anything else you would still mention is a nit. Do not report style, naming, or refactoring preferences. Cite the file and line of every finding."

note "reviewing $BASE..$HEAD_SHA in $REPO"
if timeout "$TIMEOUT_S" "$CODEX" exec -C "$REPO" -s read-only -c model_reasoning_effort=high \
     --output-schema "$SCRIPT_DIR/review-schema.json" -o "$OUT" "$PROMPT" > "$LOG" 2>&1; then
  note "result: $OUT"
else
  note "codex review failed (see $LOG)"
  exit 1
fi
