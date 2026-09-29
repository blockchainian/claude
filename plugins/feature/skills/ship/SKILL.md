---
name: ship
description: >
  Run a written plan through the codex and UX lanes to a shipped feature —
  launch the workstreams, write and run the probes, verify staging, triage the
  local review, run the fix lanes, decide production. Use when a `plan.md` exists — written in
  plan mode, by hand, or by any agent — with Workstreams, Dependencies and Checks sections:
  "/feature:ship <plan.md>", "run this plan", "ship this plan". NOT for planning, and not for a
  change small enough to do in one turn — there the launch overhead is the whole cost.
---

# Ship — run the plan, never write the code

The orchestrator is the one seat that sees both lanes. It launches, verifies and decides; it does
not implement, and it does not drive UI. Every hour it spends editing the branch is an hour the
backend lane cannot merge onto it, and every UI step it drives by hand is a step a probe would have
answered in one background call.

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
- **Checks** — exactly one command line; ship runs it in every worktree before merge and in the
  session tree after.

Optional, each read by the step that names it: **UX workstreams** (`### <id>` blocks with a
`Surfaces:` and a `Files:` line; without this section there is no UX lane), **UX checks** (one
assertion per surface, which step 4 turns into probes), **Live checks**, and any file the plan creates
marked `(new)` on its own line so the path checker skips it. `${CLAUDE_PLUGIN_ROOT}/skills/ship/minimal-plan-template.md` shows the shape. Anything
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
`outcome.md` and any `handoff.md` sit beside it; the review's `review.md` and the triaged
`findings.json` go in `<dir>/review/`.

## Workstream worktrees

Both lanes work in worktrees so nothing edits the session branch while another lane merges onto
it. `${CLAUDE_PLUGIN_ROOT}/skills/ship/workstream.sh` owns their lifecycle; never run the git
commands by hand:

- `workstream.sh open <id>` — `../.workstream-<id>` on branch `workstream/<id>` from HEAD, with
  every `node_modules` copied in (a fresh worktree carries only tracked files); prints the path.
  The first open records HEAD as the run's base. `<id>` is the plan's `### <id>`.
- `workstream.sh check <id> "<Checks command>"` — runs the plan's Checks in that worktree and
  exits with its status.
- `workstream.sh merge <id> "<Checks command>"` — merges the branch onto the session branch
  (`--no-ff`), runs Checks in the session tree, restores the branch and keeps the worktree when
  they are red, removes the worktree and branch when green, and prints the base. A conflict
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
  already has — probes, staging verify, review triage, production.
- **Name** each task for its outcome, taken verbatim from the workstream's goal in `plan.md` —
  imperative and domain-level: `Add CSV export to the reports page`, not
  `codex workstream 1`, not `UX lane A`. Keep the lane, agent, model and tool out of the subject —
  that is the "how", and `owner` already carries who. Keep ordering words out too (`after backend`,
  `step 2`); the deps carry order. Add the module when two names would collide. `activeForm` is the
  present-continuous of the same outcome (`Adding CSV export to the reports page`).
- **Wire** the plan's Dependencies as `addBlockedBy`, so the board shows what cannot start yet — a
  `needs-backend` UX workstream is blocked by the backend task it waits on; production is blocked by
  triage and the probe checkpoint.
- **Flip status** at the transitions the procedure already defines: `in_progress` when you launch a
  lane or start a checkpoint, `completed` when its branch merges clean or its verdict is green. A
  finding sent back to a lane reopens that lane's task to `in_progress` until its re-run is green.

## Procedure

1. **Check the plan and prove the check.** Read the plan, confirm it has the three
   required sections, and run the two checkers above. Confirm the tree is clean. Then run the
   plan's Checks command on this clean baseline before any fan-out: it MUST pass (exit 0). A
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

   **When a workstream finishes**, either lane: run `workstream.sh check <id> "<Checks>"`. Red goes
   back to the owner with the failing output — codex-manager `send` for a codex thread,
   `SendMessage` for a UX agent — at most twice (Hard rules); a third red is a finding for the
   user. Green: `workstream.sh merge <id> "<Checks>"`. A red post-merge check means the merged
   result breaks what each side passed alone: send the output to the owner, who fixes in the kept
   worktree, and merge again. Merge in the plan's Dependencies order; a workstream whose dependency
   is not merged yet waits. When the last workstream is merged, push the session branch and open
   the PR (`gh pr create`) or let the push update it. Merging a lane branch is integration, not
   editing.

4. **While both run, write the probes.** Turn each line of the plan's UX checks into a probe
   in the project's probe library, using its shared helpers (the project contract in the plugin
   README says where both live). This is the overlap the pipeline is built for. If no probe is
   needed, end the turn. Apart from new probe scripts, never edit the branch codex merges onto;
   leave new probes untracked or commit them before codex delivers.

5. **Review and deploy staging, both on the push.**

   a. First start the review: codex-manager `review` with `cwd` the session checkout, `base` the
      output of `workstream.sh base`, `plan` the plan's path, and `out` `<dir>/review/review.md`.
      Run its await command with `run_in_background`. It runs codex's own review mode over every
      merged workstream at once, in a read-only thread. One review per plan, one round.
   b. Then run the project's staging deploy command, read the `sha` from its JSON, and record it
      as `STAGING_SHA`.
   c. Then run the project's staging verify command and read its verdict JSON. Both exit non-zero
      on failure; gate the next step on the exit code, not on the text.
   d. Then run the plan's Live checks against staging, a minute after the deploy returns — the
      first request after a deploy can still hit the old build. Only staging runs from here —
      production is the codex lane's command.

6. **Run the UX probes against staging.** Run them with `run_in_background` and read the verdict
   JSON. Failures go back to the UX agent that owns the surface by SendMessage — it is idle, not
   dead, and keeps its context. The agent never saw your probe run, so the message carries the
   failing assertion as the verdict JSON states it (expected against actual) and the paths of the
   verdict JSON and any screenshot — never just "the probe failed". A failure still red after its
   rounds here (the cap is in Hard rules) becomes a finding in step 7.

7. **Triage, once.**

   a. When the review thread completes, read `review.md`: an overall verdict, then one finding
      per line tagged `[P0]`–`[P3]` with its file and lines and a one-paragraph body. A `failed`
      or `interrupted` completion means no review, which is a finding for the user.
   b. Verify each finding against the code — priority is the reviewer's claim, not a fact — and
      add the probe failures that survived step 6 as findings of their own. Priority orders the
      work; it never decides it.
   c. Then decide the owner of each finding, exactly once: a finding in a backend module, an API
      route or a test file is `codex` without further thought (the project contract in the plugin
      README names the paths); for the rest ask one question, does the fix change what the user
      sees or does — `ux` if yes, `codex` if no. Findings in the same file get the same owner.
   d. Write them to `<dir>/review/findings.json` as `[{file, line, claim, owner,
      disposition}]`.
   e. `disposition` starts `fixed` for a finding you keep and `rejected` — with a one-line
      `reason` — for one you verify as a false positive (it asks to revert a behaviour the plan
      calls for, it misstates the code — quote the line that shows it, the failure it describes is
      provably impossible, or it is pure style or a nit): record the rejected ones here even
      though they skip the fix round, so step 8's summary comment carries them.
   f. No kept findings means no fix round: post step 8's summary comment if a finding was
      rejected, then go to step 9.

8. **Fix, both lanes at once, one round.**

   a. `workstream.sh open fix-codex` and `workstream.sh open fix-ux`, so neither lane can dirty
      the other's tree or the session branch.
   b. codex-manager `start` in the codex worktree with the `codex` findings and the same standing
      instructions as step 2; spawn `ux-autofixer` with the `ux` findings, its worktree path and
      the UX checks. A finding touching `.claude/**` or `CLAUDE.md` comes back for the user.
   c. There is no re-review: as each lane completes, `workstream.sh check` then `merge` it as in
      step 3, push, redeploy staging if the codex lane changed anything, and re-run only the
      probes for surfaces the fixes touched.
   d. Post one PR comment summarising the findings and their dispositions; that comment is the
      review's record.

9. **Decide production, then record the outcome.** Production ships only when every finding is
   closed AND the re-run probes are green AND every test the plan's workstreams name has run and
   passed; a test that never ran or went red is a finding for the user, never a ship. Then run
   `${CLAUDE_PLUGIN_ROOT}/skills/ship/watch-ci.sh <ref> [out-file]` against the fixed head
   (the branch tip after step 8's push, or the original push when step 7 found nothing) and read
   its verdict JSON; gate on the `conclusion`
   field, never on prose. Merge only on `conclusion: success`: a plain `gh pr merge`, never
   `--admin` — a merge that would need `--admin`, or any prod, secret or infra mutation, is out of
   scope for this gate and goes to the user instead. Services that deploy from the default branch
   on merge need nothing more, and the rest go through the codex lane's deploy command (the
   project contract in the plugin README says which). Then run the plan's Live checks against production. The
   phase's record is `<dir>/outcome.md`, written from
   `${CLAUDE_PLUGIN_ROOT}/skills/ship/outcome-template.md`: what each workstream delivered and
   the test or probe that proved it, the code-review must-fix findings each as issue-tldr /
   fix-tldr / commit SHA, the PR and deploy SHAs, the live-check results, and the follow-ups left
   out of this ship. It is for the user and for memory — write it clear, succinct and fast to read,
   no code anchors. `plan.md` stays input-only; do not write an outcome into it. Then
   `workstream.sh base --clear`, write the memory files and end. Write a separate `<dir>/handoff.md` only if you must stop
   mid-phase (the context safety rail set in the user's CLAUDE.md, quota exhausted), naming exactly
   where to resume.

## Hard rules

- **Every wait ends the turn.** Launch, say one line about what is running, and stop. Task
  notifications re-invoke you when a lane finishes. NEVER idle-wait.
- **No `sleep` in the foreground.** Anything that waits runs with `run_in_background`.
- **No UI driving from the main loop.** Probes only, run with `run_in_background`, verdict JSON
  read back. Delegate to `ux-verifier` only a freeform walk with objective assertions that no probe
  can express. Judgment stays with you: judge screenshots yourself; taste calls go to the user.
- **Inconclusive is not a pass.** Fix the probe or the environment and re-run. "Probe may be stale"
  means the screen model no longer matches the app: update the probe with the deliberate change, or
  treat the mismatch itself as the finding.
- **You do not edit the branch.** Not a typo fix, not a lint fix. Findings go to the agent that owns
  the file. The one exception is the probe scripts, which no lane owns. Opening, checking and
  merging a workstream through `workstream.sh` is integration, not an edit.
- **Disjoint work runs at once.** Workstreams with disjoint `Files:` lines are launched in the same
  message, never one after another. Serial slices were the whole cost of the 2026-09-11 mobile
  session: 66 minutes of implementation took 4 h 40 min of wall clock.
- **Ad-hoc Agent spawns cannot set effort** and inherit the session's — that is why the UX lanes
  are defined agents. Once the user has raised effort, spawn defined agents only.
- **Agents are idle, not dead.** Send findings back by message and keep their context. Drop one only
  when its work is done or it has idled past the one-hour cache TTL, then spawn fresh with a short
  brief.
- **Loops are capped.** A red workstream check goes back to its owner at most twice; the third
  red is a finding for the user. A probe failure gets at most three fix rounds: two with the
  owning UX agent in step 6, then step 8's round. A review finding gets step 8's one round and no
  re-review; the re-run of the probes on the surfaces the fixes touched is the second gate. A
  finding that survives its last round goes to the user. Review priorities are unranked input;
  verify a finding before acting on it.
- **Three strikes on your own steps.** A step of yours — an inconclusive probe's re-run, the
  staging deploy or verify, `watch-ci.sh` — that fails three times in a row for the same reason
  goes to the user with its raw output. A re-run with nothing changed in between counts as a
  strike; a different failure reason restarts the count; waiting on CI or a deploy is not a
  failure. A turn that only restates status or rewrites the plan is a strike too.
- **The merge gate is CI, not prose.** `gh pr merge` runs only after `watch-ci.sh` reports
  `conclusion: success` on the fixed head. Never `--admin`. Auto-push and
  auto-merge are PR-scoped only — never a prod, secret or infra mutation from this skill; a
  finding that needs one goes to the user, not into the fix round.
- **Memory at the phase end only** — written in step 9, not mid-turn, because a memory write in
  the middle of a session invalidates the prompt cache; no handoff unless stopping mid-phase.
