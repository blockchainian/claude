---
name: implement
description: Deliver a planned feature as parallel codex workstreams off the Claude critical path — write workstreams.txt from the session's plan.md, launch implement.sh in the background (worktree pool, per-workstream checks, bounded retries, merge onto the session branch, push, PR), then relay summary.json. Use when a plan.md exists and its codex workstreams should be implemented many-at-once by codex.
---

# codex:implement — parallel codex implementation off the Claude critical path

Claude hands over once and never re-enters: git and the filesystem are the
coordination bus. Engine: `${CLAUDE_PLUGIN_ROOT}/skills/implement/implement.sh`.
Rationale and measured numbers: `${CLAUDE_PLUGIN_ROOT}/README.md`.

## When to use

A `plan.md` exists — written to the feature plugin's plan template, with one
block per codex workstream and a single Checks command — and the user wants
its backend workstreams implemented. Use for **independent, self-contained**
workstreams codex can run in parallel. NOT for research or planning (keep
steering that live), NOT for UX work (the feature plugin's UX lane owns it),
and NOT for review — that is `codex:review`.

## 1. Handover (you, ONE turn)

The input is `plan.md`, already written and committed on the session branch.
Do not re-plan and do not re-ask; the plan is the spec codex reads.

1. Check the plan is usable as a spec: each workstream block ends with the
   files it owns, no two workstreams own the same file, and Dependencies names
   any overlap the merge must expect. Conflict avoidance is the planner's job;
   the engine has no runtime check. A plan that fails this goes back to the
   planner, not into the engine.
2. Write `workstreams.txt` beside the plan: one line per codex workstream,
   each line a **pointer** into the plan, never the brief itself, for example
   `implement workstream "auth-token" per specs/<date>-<topic>/plan.md, following its Constraints and Invariants`.
   Skip the UX workstream; it is not codex's.
3. Take the check command from the plan's Checks section verbatim; the
   engine runs it in every worktree and after merge, so the plan's Invariants
   must be covered by it — the gates replace a live review.
4. Commit `workstreams.txt`. The session worktree must be CLEAN when the
   engine starts, because the run delivers onto this branch.

Launch in the same turn. Never write a handoff: plan.md and workstreams.txt
are the record, and a fresh session resumes from plan.md. The run's task
notifications bind to the session that launches it, so never `/clear`
mid-run. Set effort before the launching session starts; switching effort
mid-session invalidates the prompt cache.

```
${CLAUDE_PLUGIN_ROOT}/skills/implement/implement.sh \
  --workstreams specs/<date>-<topic>/workstreams.txt --feature <run-name> \
  --check "<the plan's Checks command>" --spec specs/<date>-<topic>/plan.md \
  [--setup "<per-worktree deps cmd>"] \
  [--concurrency N] [--retries 2] [--timeout 2400] [--runner daemon|exec] \
  [--no-push] [--deliver-wait 1800]
```

Run it in the background and END YOUR TURN — do not poll and do not read
workstream logs. Progress lives in files:
- statuses: `<repo>/.git/codex-implement/<feature>/status/workstream-*.json` and `summary.json`
- logs: `<repo>/.git/codex-implement/<feature>/logs/`

`--feature` is only the run name; no branch or worktree is created for it.
`--spec` is repo-relative and is quoted in every workstream and merge prompt.
`--setup` provisions dependencies once per worktree, because codex's sandbox
has no network: `cp -R ../main-checkout/node_modules node_modules` (`cp -Rc`
on APFS for a copy-on-write clone). The default `daemon` runner shows
workstreams and merge resolutions as rows in `codex agents`; `--runner exec`
(or `IMPLEMENT_RUNNER=exec`) uses standalone `codex exec` processes.

## 2. Execute (engine, no Claude)

- A worktree pool sized to concurrency at `../.codex-implement-<feature>/w*`,
  branched from the session branch's run-start commit, created on demand and
  removed after merge. Workstream branches are `workstreams/<feature>/<n>`.
- Per workstream: codex works in a free worktree, the engine runs `--check`,
  and commits the branch on green.
- Red = check failed, codex timeout, codex nonzero exit, or no diff. A bounded
  retry re-invokes codex in the same worktree with the failure tail appended.
- A workstream that exhausts its retries is FAILED: excluded from the merge,
  its branch kept only if it has commits, and listed in the summary and PR body.

## 3. Merge (engine, no Claude)

- Delivery takes a per-repo lock (`.git/codex-implement/deliver.lock`) so
  concurrent runs merge one at a time, and waits up to `--deliver-wait` for
  the session worktree to be clean and still on the base branch — a sibling
  session's uncommitted edits delay delivery instead of aborting it.
- Pre-merge HEAD is recorded as `refs/codex-implement/<feature>/pre-merge`
  and `.git/codex-implement/<feature>/pre-merge.sha`; `codex:review` uses it
  as the review base.
- Green workstream branches merge directly onto the session branch, in the
  session worktree. Conflicts are resolved in place by codex with the spec as
  context.
- The post-merge `--check` runs on the session branch. RED restores it with
  `git reset --keep` to the pre-merge commit and keeps every green workstream
  branch for autopsy — the worktree ends exactly where it started. The lock
  is released after this check.

## 4. Deliver (engine, no Claude)

- The session branch is pushed; the engine prints `pushed to origin`. A push
  rejected because the remote moved is retried once after merging
  `origin/<base>` in and re-running `--check` on the combination.
- If the branch has an open PR it updates; otherwise a PR is opened FROM the
  session branch to the default branch (needs `gh`; skipped on the default
  branch). `--no-push` stops after merge.
- Exit 0 = all green and delivered; 2 = partial (some workstreams failed,
  session branch green and delivered); 1 = post-merge check red (branch
  restored), merge blocked (tree stayed dirty past `--deliver-wait`, branch
  switched, or lock held too long), or push failed.

All of this is verified by `tests/test-implement.sh`.

## 5. Relay (you)

The workstreams are self-verifying: the engine gated each one and the merge
on raw exit codes. Do not re-run or re-verify them; confirm only that the
check command covered the touched surfaces.

When the run finishes, relay `summary.json` and the PR URL. FAILED
workstreams are listed in the PR body — offer to re-plan just those as a new
small run (new run name) rather than re-entering the loop yourself.

Review is not part of this skill. Under `/feature:orchestrate` the
orchestrator runs `codex:review` on the push and triages the findings;
standalone, run `codex:review` yourself when the engine reports `pushed to
origin`.

## Cleanup

None on success — workstream worktrees and branches are already gone and the
work is on the session branch. After a red post-merge check, the kept
`workstreams/<feature>/<n>` branches and `refs/codex-implement/<feature>/pre-merge`
can be deleted once the autopsy is done; `.git/codex-implement/<feature>/`
can be deleted whenever.
