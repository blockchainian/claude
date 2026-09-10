---
name: review
description: Review the commits a branch gained between two SHAs with a read-only codex exec and write the findings as JSON ({findings: [{file, line, severity, claim}]}, severity must-fix or nit) — one round, run in the background, for the caller to triage. Use after a codex:implement run reports "pushed to origin", or after the UX lane lands on a plan with no codex workstream.
---

# codex:review — structured local review of a commit range

Script: `${CLAUDE_PLUGIN_ROOT}/skills/review/review.sh`. Output schema:
`${CLAUDE_PLUGIN_ROOT}/skills/review/review-schema.json`.

## When to use

Once per plan, one round, after the push that lands the implementation:

- after `codex:implement` prints `pushed to origin`, with the base the engine
  recorded in `.git/codex-implement/<feature>/pre-merge.sha`;
- for a plan with no codex workstream, when the UX lane reports done, with
  the base the plan's recorded SHA.

Under `/feature:orchestrate` the orchestrator runs it and triages; standalone,
run it yourself. The `codex:implement` engine never calls it.

## Run

```
${CLAUDE_PLUGIN_ROOT}/skills/review/review.sh <repo> <base-sha> HEAD \
  specs/<date>-<topic>/review.json specs/<date>-<topic>/plan.md
```

Arguments: the repo checkout, the commit the changes start after (excluded),
the commit they end at (included), the output JSON path, and optionally the
repo-relative plan or spec the changes are judged against. A `.log` beside
the output receives codex's transcript. `REVIEW_TIMEOUT` (default 2400 s)
bounds the run; `IMPLEMENT_CODEX` overrides the codex binary.

Run it in the background and end your turn. It is a plain `codex exec` in a
read-only sandbox at high reasoning effort, with review instructions and the
findings schema; a finding is `must-fix` only when the change breaks
behaviour, violates a stated invariant, or leaves an input or error path
unhandled, and everything else is a `nit`. Style, naming and refactoring
preferences are not reported. `codex exec review --base` is not used because
codex-cli rejects combining `--base` with custom instructions, and no
`@codex review` comment is posted on the PR: the GitHub review is slow,
unobservable, and its threads need polling.

Exit 0 with `review.json` written; exit 1 when codex failed (the `.log` says
why). A failed review is a finding for the user, not a pass.

## Triage

Drop every `nit`. Verify each `must-fix` against the code — severity is the
reviewer's claim, not a fact. Route the survivors by scope, not severity:

- a fix confined to one file that touches no documented invariant or API
  surface is done in-session: TDD the fix, run the plan's Checks command,
  commit, push;
- a fix that spans files, touches an invariant, or whose correct shape is
  uncertain goes to a new small `codex:implement` run with its own plan,
  followed by another `codex:review` over that run's delta — re-reviewing the
  fix is the point of routing it this way.

Under `/feature:orchestrate`, the orchestrate skill's triage step owns this
routing (its fix lanes are `codex-rescue` and `ux-autofixer`); do not repeat
it there.

Verified by `tests/test-review.sh` against the stub codex in
`../implement/tests/stub-codex`.
