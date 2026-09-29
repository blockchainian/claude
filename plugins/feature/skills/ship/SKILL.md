---
name: ship
description: >
  Run a written plan through the codex and UX lanes to a shipped feature —
  launch the workstreams, merge them behind the plan's checks, deploy and check what the plan
  says to, triage codex's review, run the fix lanes, decide production. Use when a `plan.md` exists — written in
  plan mode, by hand, or by any agent — with Workstreams, Dependencies and Checks sections:
  "/feature:ship <plan.md>", "run this plan", "ship this plan". NOT for planning, and not for a
  change small enough to do in one turn — there the launch overhead is the whole cost.
---

# Ship — run the plan, never write the code

The orchestrator is the one seat that sees both lanes. It launches, verifies and decides; it does
not implement, and it does not drive UI. Every hour it spends editing the branch is an hour the
backend lane cannot merge onto it, and every UI step it drives by hand is a step a UX check would
have answered in one background call.

This skill is tuned for Opus 4.8 medium (`/model claude-opus-4-8`, `/effort medium`) in a fresh
session that reads `plan.md`; if the session differs, say so in one line and continue. A phase
boundary is a task boundary, and the planning turn's stale tool output would cost reads without
helping. Within the phase, never `/clear` for size. Effort is set once at session start. If one problem needs more, tell the user to raise
`/effort` and leave it raised for the phase: every change rewrites the whole prompt cache.

## What plan.md must contain

The plan is whatever the user wrote or approved — plan mode, a hand-written spec, any author. Ship
reads it by section, so three sections are required and the rest are read when present:

- **Workstreams** — one `### <id>` block per codex workstream, each ending with a `Files:` line
  naming the files it owns. The block plus Dependencies and Checks is the whole brief the codex
  thread receives, so it must be enough to build the workstream without the rest of the file.
- **Dependencies** — the order between workstreams, if any, and the wire contract the UX lane codes
  against. Ship wires these into the task board as `addBlockedBy`.
- **Checks** — one command per line, run in order and stopping at the first red: in every
  worktree before merge and in the session tree after.

Optional, each read by the step that names it: **UX workstreams** (`### <id>` blocks with a
`Surfaces:` and a `Files:` line; without this section there is no UX lane), **Deploy** (a
`staging:` and/or `production:` command, each printing JSON with the deployed `sha`), **UX
checks** (one probe command per line, each printing a verdict JSON and exiting non-zero on
failure), **Deploy checks** (commands run against each deploy with the value they must show), and
any file the plan creates marked `(new)` on its own line so the path checker skips it. Everything
the run needs to know about the project is in the plan; ship reads no other contract. `${CLAUDE_PLUGIN_ROOT}/skills/ship/minimal-plan-template.md` shows the shape. Anything
else in the file is context for the implementers; ship does not read it. A program that touches
many screens is one plan with many UX workstreams, not one plan per screen: planning runs once, the
implementers run at once.

Before the plan ships, and after every revision, run these two from inside the repo:

- `${CLAUDE_PLUGIN_ROOT}/skills/ship/check-paths.sh <plan> [skip-regex]` — exits 1 on a
  `MISSING:` or `AMBIGUOUS:` path; mark files the plan creates `(new)` on their own line so they
  are skipped.
- `${CLAUDE_PLUGIN_ROOT}/skills/ship/check-overlap.sh <plan>` — exits 1 when two workstreams
  list the same file, a merge conflict scheduled in advance.

A plan that fails either goes back to the user with the checker's output; ship never edits the
plan. Then grep each function, route, table, column and env var the plan names.

Everything the run writes goes next to the plan: `<dir>` below is the directory holding `plan.md`.
any `handoff.md` sits beside it; the review's `review.md` and the triaged `findings.json` go in
`<dir>/review/`.

## Workstream worktrees

Both lanes work in worktrees so nothing edits the session branch while another lane merges onto
it. `${CLAUDE_PLUGIN_ROOT}/skills/ship/workstream.sh` owns their lifecycle; never run the git
commands by hand:

- `workstream.sh open <id>` — `../.workstream-<id>` on branch `workstream/<id>` from HEAD, with
  every `node_modules` copied in (a fresh worktree carries only tracked files); prints the path.
  The first open records HEAD as the run's base. `<id>` is the plan's `### <id>`.
- `workstream.sh check <id> "<cmd>"...` — runs the plan's Checks, one argument per line, in
  that worktree, in order, and exits with the first red.
- `workstream.sh merge <id> "<cmd>"...` — merges the branch onto the session branch (`--no-ff`),
  runs Checks in the session tree, restores the branch and keeps the worktree when one is red,
  removes the worktree and branch when all are green, and prints the base. A conflict
  aborts the merge and names the files; send them to the workstream's owner to resolve in its
  worktree, then merge again.
- `workstream.sh base [--clear]` — the recorded base, for the review; cleared at phase end.

Never `Agent isolation: "worktree"` — it branches from the default branch, not from HEAD.

## Task board — the run's live view

Open a task board so the run's shape is visible while it works: one `TaskCreate` per workstream and
per checkpoint, wired with the plan's Dependencies as `addBlockedBy`, its status flipped as each
transition lands. It is a **view, not the record** — `plan.md`, the PR and the verdict JSONs stay
authoritative; the board only mirrors them and is never read back as a source of truth. Only the
orchestrator touches it — the lane agents have no Task tools and never self-report.

- **Create** the tasks in step 1, all `pending`, right after the base is pinned and the check is
  green: one per codex workstream, one per UX workstream, and one per checkpoint the procedure
  already has — review triage, UX checks, each deploy the plan names.
- **Name** each task for its outcome, taken verbatim from the workstream's goal in `plan.md` —
  imperative and domain-level: `Add CSV export to the reports page`, not
  `codex workstream 1`, not `UX lane A`. Keep the lane, agent, model and tool out of the subject —
  that is the "how", and `owner` already carries who. Keep ordering words out too (`after backend`,
  `step 2`); the deps carry order. Add the module when two names would collide. `activeForm` is the
  present-continuous of the same outcome (`Adding CSV export to the reports page`).
- **Wire** the plan's Dependencies as `addBlockedBy`, so the board shows what cannot start yet — a
  `needs-backend` UX workstream is blocked by the backend task it waits on; production is blocked by
  triage and the UX checks checkpoint.
- **Flip status** at the transitions the procedure already defines: `in_progress` when you launch a
  lane or start a checkpoint, `completed` when its branch merges clean or its verdict is green. A
  finding sent back to a lane reopens that lane's task to `in_progress` until its re-run is green.

## Procedure

1. **Check the plan and prove the check.** Read the plan, confirm it has the three
   required sections, and run the two checkers above. Confirm the tree is clean. Then run the
   plan's Checks on this clean baseline before any fan-out: they MUST pass (exit 0). A
   gate already red on the untouched tree is not a code signal — it fails every workstream identically
   and discards the whole run regardless of what the code does. Reject any such gate and send the plan
   back to fix the check (gate on a differential — new errors in touched files only — or on a command
   that passes) before launching. This one check is the cheapest guard against the most expensive
   waste; never skip it because the gate came straight from `AGENTS.md`. With the check green, open
   the task board (see **Task board**) before any fan-out, so the rest of the run is visible.

2. **Launch the codex lane — one thread per workstream.** For EVERY codex workstream in the plan,
   in one message: `workstream.sh open <id>`, then codex-manager `start` with `cwd` the printed
   path, `name` the id, and a prompt that is the workstream block verbatim plus the plan's
   Dependencies and Checks, and these standing instructions: work only in this directory, commit
   after every coherent step, run the Checks command before finishing and leave it green, never
   push, and use `ask_claude` for a question the block does not answer instead of guessing. Run
   each returned await command with `run_in_background` and end the turn. Copy any untracked env
   file a module needs into the worktree before starting the thread.

3. **Launch the UX lane, in parallel — one implementer per UX workstream.** In the same message,
   `workstream.sh open <id>` for EVERY UX workstream and spawn a `ux-implementer` agent per
   workstream with the Agent tool, giving each its worktree path, its workstream block, the wire
   contract quoted as a real response body, and the UX checks for its surfaces. They start now,
   not after codex and not after each other. Each agent commits after every coherent step and
   returns flat JSON: `status` is `done`, `blocked` or `needs-backend`.

   **When a workstream finishes**, either lane: run `workstream.sh check <id>` with the plan's
   Checks. Red goes
   back to the owner with the failing output — codex-manager `send` for a codex thread,
   `SendMessage` for a UX agent — at most twice (Hard rules); a third red is a finding for the
   user. Green: `workstream.sh merge <id>` with the same Checks. A red post-merge check means the merged
   result breaks what each side passed alone: send the output to the owner, who fixes in the kept
   worktree, and merge again. Merge in the plan's Dependencies order; a workstream whose dependency
   is not merged yet waits. When the last workstream is merged, push the session branch and open
   the PR (`gh pr create`) or let the push update it. Merging a lane branch is integration, not
   editing.

4. **Review, then deploy and check staging.** Start when the last workstream is merged and pushed.

   a. First start the review: codex-manager `review` with `cwd` the session checkout, `base` the
      output of `workstream.sh base`, `plan` the plan's path, and `out` `<dir>/review/review.md`.
      Run its await command with `run_in_background`. It runs codex's own review mode over every
      merged workstream at once, in a read-only thread. One review per plan, one round.
   b. If the plan's Deploy section has a `staging:` command, run it, read the `sha` from its JSON
      and record it as `STAGING_SHA`; then, a minute later (the first request after a deploy can
      still hit the old build), run the plan's Deploy checks against staging. Each exits non-zero
      on failure; gate on the exit code, never on the text. Without a staging command there is
      nothing to deploy yet: go on.

5. **Run the plan's UX checks.** Each line is a probe command; run them with `run_in_background`
   and read the verdict JSON. Failures go back to the UX agent that owns the surface by
   SendMessage — it is idle, not dead, and keeps its context — after `workstream.sh open
   fix-<id>`, since its own worktree went with the merge; the message names the new path, and the
   fix is checked and merged as in step 3, then staging redeployed when the plan deploys. The
   agent never saw your probe run, so the message carries the failing assertion as the verdict JSON
   states it (expected against actual) and the paths of the verdict JSON and any screenshot — never
   just "the probe failed". A failure still red after its rounds here (the cap is in Hard rules)
   becomes a finding in step 6. No UX checks in the plan means this step is skipped.

6. **Triage, once.**

   a. When the review thread completes, read `review.md`: an overall verdict, then one finding
      per line tagged `[P0]`–`[P3]` with its file and lines and a one-paragraph body. A `failed`
      or `interrupted` completion means no review, which is a finding for the user.
   b. Verify each finding against the code — priority is the reviewer's claim, not a fact — and
      add the UX-check failures that survived step 5 as findings of their own. Priority orders
      the work; it never decides it.
   c. Then decide the owner of each finding, exactly once, from the plan: a file on a UX
      workstream's `Files:` line is `ux`, any other file is `codex`; a finding with no file goes
      to whichever lane owns the behaviour it describes. Findings in the same file get the same
      owner.
   d. Write them to `<dir>/review/findings.json` as `[{file, line, claim, owner,
      disposition}]`.
   e. `disposition` starts `fixed` for a finding you keep and `rejected` — with a one-line
      `reason` — for one you verify as a false positive (it asks to revert a behaviour the plan
      calls for, it misstates the code — quote the line that shows it, the failure it describes is
      provably impossible, or it is pure style or a nit): record the rejected ones here even
      though they skip the fix round, so step 7's summary comment carries them.
   f. No kept findings means no fix round: post step 7's summary comment if a finding was
      rejected, then go to step 8.

7. **Fix, both lanes at once, one round.**

   a. `workstream.sh open fix-codex` and `workstream.sh open fix-ux`, so neither lane can dirty
      the other's tree or the session branch.
   b. codex-manager `start` in the codex worktree with the `codex` findings and the same standing
      instructions as step 2; spawn `ux-autofixer` with the `ux` findings, its worktree path and
      the UX checks for its surfaces. A finding touching `.claude/**` or `CLAUDE.md` comes back
      for the user.
   c. There is no re-review: as each lane completes, `workstream.sh check` then `merge` it as in
      step 3 and push; if the plan deploys to staging, redeploy it and re-run its Deploy checks;
      re-run only the UX checks for surfaces the fixes touched.
   d. Post one PR comment summarising the findings and their dispositions; that comment is the
      review's record. Without a PR, the summary goes into your final message.

8. **Decide production, then write the record.** Production ships only when every finding is
   closed AND the re-run UX checks are green AND every test the plan's workstreams name has run
   and passed; a test that never ran or went red is a finding for the user, never a ship. When
   the branch has a PR, run `${CLAUDE_PLUGIN_ROOT}/skills/ship/watch-ci.sh <ref> [out-file]`
   against the fixed head (the branch tip after step 7's push, or the original push when step 6
   found nothing) and read its verdict JSON; gate on the `conclusion` field, never on prose. Merge
   only on `conclusion: success`: a plain `gh pr merge`, never `--admin` — a merge that would need
   `--admin`, or any prod, secret or infra mutation, is out of scope for this gate and goes to the
   user instead. A repo with no remote ends at the merged session branch. If the plan's Deploy
   section has a `production:` command, run it after the merge and then the plan's Deploy checks
   against production; without one, the merge is the ship. The PR description is the phase's
   record: before merging, write it (or rewrite it) with what each workstream delivered and the
   test or check that proved it, the review findings each as issue-tldr / fix-tldr / commit SHA,
   the deploy SHAs and deploy-check results, and the follow-ups left out of this ship — clear,
   succinct, no code anchors. Without a PR the same goes in your final message. `plan.md` stays
   input-only; do not write the record into it. Then `workstream.sh base --clear`, write the
   memory files and end. Write a separate
   `<dir>/handoff.md` only if you must stop mid-phase (the context safety rail set in the user's
   CLAUDE.md, quota exhausted), naming exactly where to resume.

## Hard rules

- **Every wait ends the turn.** Launch, say one line about what is running, and stop. Task
  notifications re-invoke you when a lane finishes. NEVER idle-wait.
- **No `sleep` in the foreground.** Anything that waits runs with `run_in_background`.
- **No UI driving from the main loop.** The plan's UX checks only, run with `run_in_background`,
  verdict JSON read back. Delegate to `ux-verifier` only a freeform walk with objective assertions
  that no check can express. Judgment stays with you: judge screenshots yourself; taste calls go to
  the user.
- **Inconclusive is not a pass.** Fix the environment and re-run. A check that no longer matches
  the app is a finding for the user, who owns the plan; never rewrite a check to make it pass.
- **You do not edit the branch.** Not a typo fix, not a lint fix, not a check. Findings go to the
  agent that owns the file. Opening, checking and merging a workstream through `workstream.sh` is
  integration, not an edit.
- **Disjoint work runs at once.** Workstreams with disjoint `Files:` lines are launched in the same
  message, never one after another. Serial slices were the whole cost of the 2026-09-11 mobile
  session: 66 minutes of implementation took 4 h 40 min of wall clock.
- **Ad-hoc Agent spawns cannot set effort** and inherit the session's — that is why the UX lanes
  are defined agents. Once the user has raised effort, spawn defined agents only.
- **Agents are idle, not dead.** Send findings back by message and keep their context. Drop one only
  when its work is done or it has idled past the one-hour cache TTL, then spawn fresh with a short
  brief.
- **Loops are capped.** A red workstream check goes back to its owner at most twice; the third
  red is a finding for the user. A UX-check failure gets at most three fix rounds: two with the
  owning UX agent in step 5, then step 7's round. A review finding gets step 7's one round and no
  re-review; the re-run of the UX checks on the surfaces the fixes touched is the second gate. A
  finding that survives its last round goes to the user. Review priorities are unranked input;
  verify a finding before acting on it.
- **Three strikes on your own steps.** A step of yours — an inconclusive check's re-run, a
  deploy, `watch-ci.sh` — that fails three times in a row for the same reason
  goes to the user with its raw output. A re-run with nothing changed in between counts as a
  strike; a different failure reason restarts the count; waiting on CI or a deploy is not a
  failure. A turn that only restates status or rewrites the plan is a strike too.
- **The merge gate is CI, not prose.** `gh pr merge` runs only after `watch-ci.sh` reports
  `conclusion: success` on the fixed head. Never `--admin`. Auto-push and
  auto-merge are PR-scoped only — never a prod, secret or infra mutation from this skill; a
  finding that needs one goes to the user, not into the fix round.
- **Memory at the phase end only** — written in step 9, not mid-turn, because a memory write in
  the middle of a session invalidates the prompt cache; no handoff unless stopping mid-phase.
