---
name: ship
description: >
  Run a written plan through the codex and UX lanes to a shipped feature —
  launch the workstreams, write and run the probes, verify staging, triage the
  local review, run the fix lanes, decide production. Use when a `plan.md` exists and has passed
  the path checker: "/feature:ship <plan.md>", "run this plan", "ship this
  plan". NOT for planning, and not for a change small enough to do in one
  turn — there the launch overhead is the whole cost.
---

# Ship — run the plan, never write the code

The orchestrator is the one seat that sees both lanes. It launches, verifies and decides; it does
not implement, and it does not drive UI. Every hour it spends editing the branch is an hour the
backend lane cannot merge onto it, and every UI step it drives by hand is a step a probe would have
answered in one background call.

This skill is tuned for Opus 4.8 medium (`/model claude-opus-4-8`, `/effort medium`) in a fresh
session that reads `plan.md` and its `problem.md`; if the session differs, say so in one line and
continue. A phase boundary is a task boundary, and the grounding sweep's stale tool output would
cost reads without helping. Within the phase, never `/clear` for size. Effort is set once at session start. If one problem needs more, tell the user to raise
`/effort` and leave it raised for the phase: every change rewrites the whole prompt cache.

## What plan.md must contain

The plan follows `${CLAUDE_PLUGIN_ROOT}/skills/ship/plan-template.md`: Scope, Facts, Constraints,
Workstreams, UX workstreams, Dependencies, Invariants, UX checklist per surface, New files, Checks,
Live checks. It is agent-facing only — no decision rationale, no shipped outcome. A program that
touches many screens is one plan with many UX workstreams, not one plan per screen: grounding and
planning run once, the implementers run at once. Each workstream block is the implementer's whole
brief: `codex:implement` passes the plan as the spec and writes one pointer line per workstream
that scopes the agent to the shared core plus its block, adding no facts. The `planner` agent
(`${CLAUDE_PLUGIN_ROOT}/agents/planner.md`) writes it, and beside it `decisions.md` —
the pre-launch human gate (rationale, rejected alternatives, risks, open questions) that no agent
reads and the user reviews before you launch.

Before the plan ships, and after every revision, run these three from inside the repo:

- `${CLAUDE_PLUGIN_ROOT}/skills/ground/check-paths.sh <plan> [skip-regex]` — exits 1 on a
  `MISSING:` or `AMBIGUOUS:` path; mark files the plan creates `(new)` on their own line so they
  are skipped.
- `${CLAUDE_PLUGIN_ROOT}/skills/ground/check-overlap.sh <plan>` — exits 1 when two workstreams
  list the same file, a merge conflict scheduled in advance.
- `${CLAUDE_PLUGIN_ROOT}/skills/ground/check-acceptance.py <plan> <problem.md>` — exits 1 on
  `UNCOVERED: AC<n>`, an acceptance criterion no test references, which sends the plan back to
  the planner.

Then grep each function, route, table, column and env var the plan names.

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

1. **Confirm the decisions gate ran, pin the base, and prove the check.** First confirm the plan's
   `decisions.md` carries a `Gate: passed` top line — the record that `/feature:plan` gated the
   decisions, open questions and risks with the user. If it is absent, stop and tell the user to run
   `/feature:plan <problem.md>` first; ship never gates decisions itself. Then read the plan and
   compare `git rev-parse --short HEAD` against
   the SHA the plan records. If they differ, rebase onto the plan's base or re-check the plan's paths
   before launching — a plan is valid only at its SHA. Confirm the tree is clean. Then run each
   workstream's `--check` command on this clean baseline before any fan-out: it MUST pass (exit 0). A
   gate already red on the untouched tree is not a code signal — it fails every workstream identically
   and discards the whole run regardless of what the code does. Reject any such gate and send the plan
   back to fix the check (gate on a differential — new errors in touched files only — or on a command
   that passes) before launching. This one check is the cheapest guard against the most expensive
   waste; never skip it because the gate came straight from `AGENTS.md`. With the base pinned and the
   check green, open the task board (see **Task board**) before any fan-out, so the rest of the run
   is visible.

2. **Launch the backend lane.** Invoke the `codex:implement` skill with the plan's codex workstreams
   (backend; frontend only when the plan has no UX lane). It runs in the background, verifies
   itself per workstream and post-merge on raw exit codes, merges onto the session branch and
   pushes a PR. It does not review; that is step 5. Do not re-verify its
   workstreams; confirm instead that its check command covers the touched surfaces.

3. **Launch the UX lane, in parallel — one implementer per UX workstream.** Spawn a
   `ux-implementer` agent with the Agent tool for EVERY UX workstream in the plan, all in the same
   message, giving each its workstream block, the wire contract quoted as a real response body,
   and the UX checklist for its surfaces. They start now, not after codex and not after each
   other. One workstream works on the session branch. Each further workstream gets its own tree
   first (the worktree recipe below, `../.ux-<id>` on branch `ux/<id>` from HEAD), and its brief
   names that path; when its agent reports `done`, merge `ux/<id>` onto the session branch
   (`git merge --no-ff ux/<id>`; the plan's disjoint `Files:` lines make it clean) and remove the
   tree. Merging a lane branch is integration, not editing. Each agent commits after every
   coherent step and returns flat JSON: `status` is `done`, `blocked` or `needs-backend`.

   **Worktree recipe.** `git worktree add ../.ux-<name> -b ux/<name> <base>` then
   `cp -Rc <module>/node_modules ../.ux-<name>/<module>/node_modules` for the module the lane
   touches (about 40 s; clone, never symlink — the codex sandbox writes through symlinks; copy any
   untracked env file the module needs the same way). Remove with
   `git worktree remove --force ../.ux-<name>` and `git branch -D ux/<name>` once merged, in the
   background (about 15 s). Never `Agent isolation: "worktree"` — it branches from the default
   branch, not from HEAD.

4. **While both run, write the probes.** Turn each line of the plan's UX checklist into a probe
   in the project's probe library, using its shared helpers (the project contract in the plugin
   README says where both live). This is the overlap the pipeline is built for. If no probe is
   needed, end the turn. Apart from new probe scripts, never edit the branch codex merges onto;
   leave new probes untracked or commit them before codex delivers.

5. **Review and deploy staging, both on the push.**

   a. Watch `implement.sh`'s output with `Monitor` for the line `pushed to origin` and act on it,
      not on the run's exit.
   b. First start the review in the background: invoke the `codex:review` skill, which names its
      script, and run it as `review.sh <repo> <base> HEAD specs/<date>-<topic>/review.json
      <plan.md>`, where `<base>` is the
      `.git/codex-implement/<feature>/pre-merge.sha` the script recorded. A plan with no codex
      workstream has no `implement.sh` run: start the same command when the UX lane reports
      `done`, with `<base>` the plan's base SHA. One review per plan, one round.
   c. Then run the project's staging deploy command, read the `sha` from its JSON, and record it
      as `STAGING_SHA`.
   d. Then run the project's staging verify command and read its verdict JSON. Both exit non-zero
      on failure; gate the next step on the exit code, not on the text.
   e. Then run the plan's Live checks against staging, a minute after the deploy returns — the
      first request after a deploy can still hit the old build. Only staging runs from here —
      production is the codex lane's command.

6. **Run the UX probes against staging.** Run them with `run_in_background` and read the verdict
   JSON. Failures go back to the UX agent that owns the surface by SendMessage — it is idle, not
   dead, and keeps its context. A failure still red after its rounds here (the cap is in Hard
   rules) becomes a finding in step 7.

7. **Triage, once.**

   a. When the review has written `review.json` (its process exits; a fail exit means no review,
      which is a finding for the user), drop every `nit`.
   b. Verify each `must-fix` against the code — severity is the reviewer's claim, not a fact —
      and add the probe failures that survived step 6 as findings of their own.
   c. Then decide the owner of each finding, exactly once: a finding in a backend module, an API
      route or a test file is `codex` without further thought (the project contract in the plugin
      README names the paths); for the rest ask one question, does the fix change what the user
      sees or does — `ux` if yes, `codex` if no. Findings in the same file get the same owner.
   d. Write them to `specs/<date>-<topic>/findings.json` as `[{file, line, claim, owner,
      disposition}]`.
   e. `disposition` starts `fixed` for a finding you keep and `rejected` — with a one-line
      `reason` — for a must-fix you verify as a false positive (it asks to revert an intended
      change, or the code already handles it): record the rejected ones here even though they
      skip the fix round, so step 8's summary comment carries them.
   f. No kept findings means no fix round: post step 8's summary comment if a finding was
      rejected, then go to step 9.

8. **Fix, both lanes at once, one round.**

   a. Give the UX lane its own tree so neither lane can dirty the other's: the worktree recipe
      from step 3 with `../.ux-<branch>`, branch `ux/<branch>`, base `origin/<branch>`, for the
      module the UX findings touch (another module's only for a finding there).
   b. Spawn `codex:codex-rescue` with the `codex` findings, told to work in the session checkout,
      commit, rebase onto the remote branch and push; spawn `ux-autofixer` with the `ux`
      findings, the worktree path, the side branch `ux/<branch>`, the PR branch and the UX
      checklist. A finding touching `.claude/**` or `CLAUDE.md` comes back for the user.
   c. There is no re-review: when both lanes are done, redeploy staging if the codex lane pushed,
      re-run only the probes for surfaces the fixes touched, then remove the tree.
   d. Post one PR comment summarising the findings and their dispositions; that comment is the
      review's record.

9. **Decide production, then record the outcome.** Production ships only when every finding is
   closed AND the re-run probes are green AND every acceptance criterion is met — the gate's
   `[AC<n>]`-tagged tests pass and each `ui` criterion's probe is green; a criterion whose test
   never ran or went red is an unmet acceptance, a finding for the user, never a ship. This is
   what makes the shipped feature equal the definition, not a half-shipped mixed feature. Then run
   `${CLAUDE_PLUGIN_ROOT}/skills/ship/watch-ci.sh <ref> [out-file]` against the fixed head
   (the branch tip after step 8's pushes, or the original push when step 7 found nothing) and read
   its verdict JSON; gate on the `conclusion`
   field, never on prose. Merge only on `conclusion: success`: a plain `gh pr merge`, never
   `--admin` — a merge that would need `--admin`, or any prod, secret or infra mutation, is out of
   scope for this gate and goes to the user instead. Services that deploy from the default branch
   on merge need nothing more, and the rest go through the codex lane's deploy command (the
   project contract in the plugin README says which). Then run the plan's Live checks against production. The
   phase's record is `specs/<date>-<topic>/outcome.md`, written from
   `${CLAUDE_PLUGIN_ROOT}/skills/ship/outcome-template.md`: the per-`AC<n>` acceptance verdict and
   the test or probe that settled each, the code-review must-fix findings each as issue-tldr /
   fix-tldr / commit SHA, the PR and deploy SHAs, the live-check results, and the follow-ups left
   out of this ship. It is for the user and for memory — write it clear, succinct and fast to read,
   no code anchors. `plan.md` stays input-only; do not write an outcome into it. Then write the
   memory files and end. Write a separate `specs/<date>-<topic>/handoff.md` only if you must stop
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
  the file. The one exception is the probe scripts, which no lane owns. Creating and removing a
  lane's worktree, and merging its branch when it reports done, is integration, not an edit.
- **Disjoint work runs at once.** Workstreams with disjoint `Files:` lines are launched in the same
  message, never one after another. Serial slices were the whole cost of the 2026-09-11 mobile
  session: 66 minutes of implementation took 4 h 40 min of wall clock.
- **Ad-hoc Agent spawns cannot set effort** and inherit the session's — that is why the UX lanes
  are defined agents. Once the user has raised effort, spawn defined agents only.
- **Agents are idle, not dead.** Send findings back by message and keep their context. Drop one only
  when its work is done or it has idled past the one-hour cache TTL, then spawn fresh with a short
  brief.
- **Loops are capped.** A probe failure gets at most three fix rounds: two with the owning UX
  agent in step 6, then step 8's round. A review finding gets step 8's one round and no
  re-review; the re-run of the probes on the surfaces the fixes touched is the second gate. A
  finding that survives its last round goes to the user. Review severity labels are unranked
  input; verify a finding before acting on it.
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
