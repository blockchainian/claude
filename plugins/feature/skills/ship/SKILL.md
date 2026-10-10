---
name: ship
description: >
  Run a written plan through the codex and UX workstreams to a shipped feature —
  launch the workstreams, merge them behind the plan's checks, deploy and check what the plan
  says to, triage codex's review, run the fix workstreams, decide production. Use when a `plan.md` exists — written in
  plan mode, by hand, or by any agent — with Workstreams, Dependencies and Checks sections:
  "/feature:ship <plan.md>", "run this plan", "ship this plan". NOT for planning, and not for a
  change small enough to do in one turn — there the launch overhead is the whole cost.
---

# Ship

You are the orchestrator, the one seat that sees both the codex and the UX workstreams. You
launch, verify and decide; you never implement and never drive UI. Every hour you spend editing
the branch is an hour no workstream can merge onto it, and every UI step you drive by hand is one
a UX check answers in a single background call.

The skill is tuned for Opus 5.5 medium (`/model claude-opus-5-5`, `/effort medium`) in a fresh
session that reads `plan.md`, because a phase boundary is a task boundary and the planning turn's
stale tool output only costs reads. If the session differs, say so in one line and continue.
Within the phase, never `/clear` for size. Effort is set once at session start; if one problem
needs more, tell the user to raise `/effort` and leave it raised for the phase, since every change
rewrites the whole prompt cache.

## Skill directory

Set `SKILL_DIR="${CLAUDE_PLUGIN_ROOT}/skills/ship"` in every shell call; `workstream.sh` below
means `$SKILL_DIR/scripts/workstream.sh`. Run every script from inside the repo.

## Environment variables

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `CI_TIMEOUT` | Seconds `watch-ci.sh` waits for checks before reporting `state: timeout`; default 300 | Optional | The command's environment |
| `CI_INTERVAL` | Seconds between `watch-ci.sh` polls; default 5 | Optional | The command's environment |

## The plan

The plan is whatever the user wrote or approved, by any author. Ship reads it by section and reads
no other contract; anything else in the file is context for the implementers.
`$SKILL_DIR/minimal-plan-template.md` shows the shape. Three sections are required:

- **Workstreams**: one `### <id>` block per codex workstream, each ending with a `Files:` line
  naming the files it owns. The block plus Dependencies and Checks is the codex thread's whole
  brief, so it must be enough to build the workstream without the rest of the file.
- **Dependencies**: the order between workstreams, if any, and the wire contract the UX
  workstream codes against.
- **Checks**: one command per line, run in order and stopping at the first red, by each
  workstream before finishing and once in the session tree after each merged batch.

The optional sections are each read by the step that names them:

- **UX workstreams**: `### <id>` blocks with a `Surfaces:` and a `Files:` line. Without this
  section there is no UX workstream.
- **Deploy**: a `staging:` and/or `production:` command, each printing JSON with the deployed
  `sha`.
- **UX checks**: one probe command per line, each printing a verdict JSON and exiting non-zero on
  failure.
- **Deploy checks**: commands run after each deploy, with `DEPLOY_ENV` and the deploy JSON's
  fields in the environment.

Any file the plan creates is marked `(new)` on its own line so the path checker skips it. A
program that touches many screens is one plan with many UX workstreams, not one plan per screen:
planning runs once and the implementers run at once.

Run both checkers before the plan ships and after every revision:

- `$SKILL_DIR/scripts/check-paths.sh <plan> [skip-regex]` exits 1 on a `MISSING:` or
  `AMBIGUOUS:` path.
- `$SKILL_DIR/scripts/check-overlap.sh <plan>` exits 1 when two workstreams list the same file,
  which is a merge conflict scheduled in advance.

A plan that fails either goes back to the user with the checker's output; you never edit the plan.
Then grep each function, route, table, column and env var the plan names.

Everything the run writes goes next to the plan. `<dir>` is the directory holding `plan.md`; any
`handoff.md` sits beside it, and the review's `review.md`, the triaged `findings.json` and the
run's `decisions.md` go in `<dir>/review/`.

## Task board

Keep a task board so the run's shape is visible while it works: one `TaskCreate` per workstream
and per checkpoint, wired with the plan's Dependencies as `addBlockedBy`. The board is a view, not
the record: `plan.md`, the PR and the verdict JSONs stay authoritative, and the board is never
read back as a source of truth. Only you touch it; the workstream agents have no Task tools and
never self-report.

- **Create** the tasks in "Check the plan and prove the check", all `pending`, right after the base
  is pinned and the check is green: one per codex workstream, one per UX workstream, and one per
  checkpoint the procedure has (review triage, UX checks, each deploy the plan names).
- **Name** each task for its outcome, verbatim from the workstream's goal in `plan.md`, imperative
  and domain-level: `Add CSV export to the reports page`, not `codex workstream 1` or
  `UX workstream A`. Keep the workstream, agent, model and tool out of the subject, since `owner`
  carries who, and keep out ordering words (`after backend`, `step 2`), since the deps carry order.
  Add the module when two names would collide. `activeForm` is the present continuous of the same
  outcome (`Adding CSV export to the reports page`).
- **Wire** the Dependencies as `addBlockedBy`, so the board shows what cannot start yet: a codex
  workstream waiting for another one's merge, a `needs-backend` UX workstream blocked by the
  backend task it waits on. Production is blocked by triage and the UX checks checkpoint.
- **Flip status** to `in_progress` when you launch a workstream or start a checkpoint, and to
  `completed` when its branch merges clean or its verdict is green. A finding sent back to a
  workstream reopens its task to `in_progress` until its re-run is green.

## Check the plan and prove the check

Read the plan, confirm it has the three required sections, run both checkers, and confirm the tree
is clean. Then run the plan's Checks on this clean baseline before any fan-out: they MUST pass
(exit 0). A gate already red on the untouched tree is not a code signal; it fails every workstream
identically and discards the whole run whatever the code does. Reject such a gate and send the
plan back to fix the check, either gating on a differential (new errors in touched files only) or
on a command that passes. This is the cheapest guard against the most expensive waste; never skip
it because the gate came straight from `AGENTS.md`. With the check green, open the task board
before any fan-out.

## Launch the codex workstreams

Give every codex workstream whose Dependencies are already merged with their batch's Checks green
(none, at the start) its own thread, all in one message: `workstream.sh open <id>`, then
codex-manager `start` with `cwd` the printed path, `name` the id, and a prompt made of the
workstream block verbatim, the plan's Dependencies and Checks, and
`$SKILL_DIR/codex-standing-instructions.md` verbatim. Copy any untracked env file a module needs
into the worktree before starting the thread. Run each returned await command with
`run_in_background` and end the turn.

The thread's last message arrives in the completed event as flat JSON. Check its `decisions`
marked unconfirmed against the plan, then append them all to `<dir>/review/decisions.md` under a
`## <id>` heading, one per line as the thread wrote them; its `findings` join the triage. That
file is the one place the rules decided during the run live: the reviewer, the fix round and the
PR description all read it, and the user decides afterwards which belong in the plan.

A workstream the Dependencies put after another is opened and started the same way, in the
message where that dependency's batch passes Checks. `open` branches from the merged HEAD, so its
worktree already carries what it depends on and its Checks can pass alone.

## Launch the UX workstreams

In the same message, run `workstream.sh open <id>` for EVERY UX workstream and spawn one
`ux-implementer` agent per workstream with the Agent tool. Give each its worktree path, its
workstream block, the wire contract quoted as a real response body, the plan's Checks to run
before finishing, and the UX checks for its surfaces. They start now, not after codex and not
after each other. Each agent commits after every coherent step and returns flat JSON whose
`status` is `done`, `blocked` or `needs-backend`.

## Merge finished workstreams

When a codex or UX workstream finishes, merge it straight away with `workstream.sh merge <id>` and
no commands; do not repeat its Checks with `workstream.sh check`. Merge the workstreams that
finished together as one batch, in Dependencies order; one whose dependency is not merged yet
waits. Run the plan's Checks once in the session tree after the batch, without waiting for
workstreams still running.

On red, find the merged workstream that caused it, run `workstream.sh open fix-<id>`, and send its
owner the new path and the failing output (codex-manager `send` or `SendMessage`) to fix and run
Checks there. Merge the fix without commands and re-run Checks in the session tree, within the
loop caps in "Limits". On green, open dependent workstreams as in "Launch the codex workstreams".
When the last batch is green, push the session branch and open the PR (`gh pr create`) or let the
push update it.

## Review and deploy to staging

Start when the last workstream is merged and pushed. First start the review: codex-manager
`review` with `cwd` the session checkout, `base` the output of `workstream.sh base`, `plan` the
plan's path, `decisions` the path of `<dir>/review/decisions.md` when the run wrote one, and
`out` `<dir>/review/review.md`. Run its await command with `run_in_background`. It runs codex's
own review mode over every merged workstream at once, in a read-only thread; there is one review
per plan, one round.

If the Deploy section has a `staging:` command, run it and record the `sha` from its JSON as
`STAGING_SHA`. A minute later, since the first request after a deploy can still hit the old build,
run the Deploy checks against staging. Each runs with `DEPLOY_ENV` set to the deploy line's label
(`staging`) and every top-level string field of the deploy JSON exported as `DEPLOY_<FIELD>` in
upper case (`DEPLOY_SHA` always, `DEPLOY_URL` when the command printed a `url`, anything else the
project adds). Each exits non-zero on failure; gate on the exit code, never on the text. Without
a staging command there is nothing to deploy yet, so go on.

## Run the UX checks

Skip this when the plan has no UX checks. Otherwise run each probe command with
`run_in_background` and read its verdict JSON. Send each failure by `SendMessage` to the UX agent
that owns the surface; it is idle, not dead, and keeps its context. First run `workstream.sh open
fix-<id>`, since its own worktree went with the merge, and name the new path in the message. The
agent never saw your probe run, so the message carries the failing assertion as the verdict JSON
states it (expected against actual) and the paths of the verdict JSON and any screenshot, never
just "the probe failed". The owner runs Checks before finishing, the fix merges in a batch as in
"Merge finished workstreams", and staging is redeployed when the plan deploys. A failure still red
after its rounds here becomes a finding in triage.

## Triage

Triage runs once. When the review thread completes, read `review.md`: an overall verdict, then one
finding per line tagged `[P0]` to `[P3]` with its file and lines and a one-paragraph body. A
`failed` or `interrupted` completion means there is no review, which is a finding for the user.

Verify each finding against the code, because priority is the reviewer's claim, not a fact; it
orders the work but never decides it. Add the UX-check failures that survived "Run the UX checks"
as findings of their own. Then decide each finding's owner exactly once, from the plan: a file on
a UX workstream's `Files:` line is `ux`, any other file is `codex`, and a finding with no file
goes to whichever workstream owns the behaviour it describes. Findings in the same file get the
same owner.

Write them to `<dir>/review/findings.json` as `[{file, line, claim, owner, disposition}]`.
`disposition` starts as `fixed` for a finding you keep, and `rejected` with a one-line `reason`
for one you verify as a false positive: it asks to revert a behaviour the plan calls for, it
misstates the code (quote the line that shows it), the failure it describes is provably
impossible, or it is pure style or a nit. Record rejected findings too, so the fix round's summary
comment carries them. With no kept findings there is no fix round: post the summary comment if a
finding was rejected, then decide production.

## Fix

Codex and UX fix at once, in one round. Run `workstream.sh open fix-codex` when there are `codex`
findings and `workstream.sh open fix-ux` when there are `ux` findings, so neither can dirty the
other's tree or the session branch. Start codex-manager in the codex worktree with the `codex`
findings, the path of `decisions.md` and the same standing instructions, and append what it
decides to that file as before. Spawn `ux-autofixer` with the `ux` findings, its worktree path,
the plan's Checks to run before finishing and the UX checks for its surfaces. A finding touching
`.claude/**` or `CLAUDE.md` comes back for the user.

There is no re-review. Merge completed workstreams without commands, run Checks once per merged
batch, and push only on green. If the plan deploys to staging, redeploy and re-run its Deploy
checks, and re-run only the UX checks for surfaces the fixes touched. Post one PR comment
summarising the findings and their dispositions; that comment is the review's record. Without a
PR, the summary goes into your final message.

## Decide production

Production ships only when every finding is closed AND the re-run UX checks are green AND every
test the plan's workstreams name has run and passed; a test that never ran or went red is a
finding for the user, never a ship.

When the branch has a PR, run `$SKILL_DIR/scripts/watch-ci.sh <ref> [out-file]` against the fixed
head (the branch tip after the fix round's push, or the original push when triage kept nothing)
and gate on its verdict JSON's `conclusion` field, never on prose. Merge only on
`conclusion: success`, with a plain `gh pr merge`, never `--admin`. A merge that would need
`--admin`, or any prod, secret or infra mutation, is out of scope for this gate and goes to the
user. A repo with no remote ends at the merged session branch.

Before merging, write or rewrite the PR description, which is the phase's record: what each
workstream delivered and the test or check that proved it, the rules in `decisions.md` the plan
did not state, each review finding as issue-tldr / fix-tldr / commit SHA, the deploy SHAs and
deploy-check results, and the follow-ups left out of this ship, clear and succinct with no code
anchors. Without a PR the same goes in your final message. `plan.md` stays input-only; never write
the record into it.

If the Deploy section has a `production:` command, run it after the merge, then the Deploy checks
against production with `DEPLOY_ENV=production` and the deploy JSON's fields exported as for
staging; without one, the merge is the ship. Then run `workstream.sh base --clear` and end. Write
`<dir>/handoff.md` only if you must stop mid-phase (the context safety rail in the user's
CLAUDE.md, quota exhausted), naming exactly where to resume.

## Workstream worktrees

Codex and UX workstreams work in worktrees so nothing edits the session branch while another
workstream merges onto it. `$SKILL_DIR/scripts/workstream.sh` owns their lifecycle; never run the
git commands by hand, and never use `Agent isolation: "worktree"`, which branches from the default
branch, not from HEAD.

- `workstream.sh open <id>` creates `../.workstream-<id>` on branch `workstream/<id>` from HEAD,
  copies in every `node_modules` (a fresh worktree carries only tracked files) and prints the path.
  The first open records HEAD as the run's base. `<id>` is the plan's `### <id>`.
- `workstream.sh check <id> "<cmd>"...` runs the plan's Checks, one argument per line, in that
  worktree in order, and exits with the first red.
- `workstream.sh merge <id> ["<cmd>"...]` merges the branch onto the session branch (`--no-ff`),
  removes the worktree and branch, and prints the base. With commands, it runs them in the session
  tree first, restoring the branch and keeping the worktree when one is red. A conflict aborts the
  merge and names the files; send them to the workstream's owner to resolve in its worktree, then
  merge again.
- `workstream.sh base [--clear]` prints the recorded base for the review; clear it at phase end.

Opening, checking and merging a workstream through `workstream.sh` is integration, not editing.

## Rules

- **Every wait ends the turn.** Launch, say one line about what is running, and stop; task
  notifications re-invoke you when a workstream finishes. NEVER idle-wait, and never `sleep` in the
  foreground: anything that waits runs with `run_in_background`.
- **No UI driving from the main loop.** Run only the plan's UX checks, in the background, and read
  their verdict JSON back. Delegate to `ux-verifier` only a freeform walk with objective assertions
  no check can express. Judgment stays with you: judge screenshots yourself, and leave taste calls
  to the user.
- **Inconclusive is not a pass.** Fix the environment and re-run. A check that no longer matches
  the app is a finding for the user, who owns the plan; never rewrite a check to make it pass.
- **You do not edit the branch.** Not a typo fix, not a lint fix, not a check. Findings go to the
  agent that owns the file.
- **Disjoint work runs at once.** Workstreams with disjoint `Files:` lines are launched in the same
  message, never one after another.
- **Ad-hoc Agent spawns cannot set effort** and inherit the session's, which is why the UX
  workstreams are defined agents. Once the user has raised effort, spawn defined agents only.
- **Agents are idle, not dead.** Send findings back by message and keep their context. Drop an
  agent only when its work is done or it has idled past the one-hour cache TTL, then spawn a fresh
  one with a short brief.
- **The merge gate is CI, not prose.** `gh pr merge` runs only after `watch-ci.sh` reports
  `conclusion: success` on the fixed head. Auto-push and auto-merge are PR-scoped only: never a
  prod, secret or infra mutation from this skill, and a finding that needs one goes to the user,
  not into the fix round.
- **No handoff unless stopping mid-phase.**

## Limits

- A red batch Check goes back to the responsible workstream's owner at most twice; the third red
  is a finding for the user.
- A UX-check failure gets at most three fix rounds: two with the owning UX agent in "Run the UX
  checks", then the fix round.
- A review finding gets the fix round's one round and no re-review; re-running the UX checks on
  the surfaces the fixes touched is the second gate.
- A finding that survives its last round goes to the user. Review priorities are unranked input;
  verify a finding before acting on it.
- A step of yours (an inconclusive check's re-run, a deploy, `watch-ci.sh`) that fails three times
  in a row for the same reason goes to the user with its raw output. A re-run with nothing changed
  in between counts as a strike, a different failure reason restarts the count, and waiting on CI
  or a deploy is not a failure. A turn that only restates status or rewrites the plan is a strike
  too.
