---
name: orchestrate
description: >
  Run a written plan through the codex and UX lanes to a shipped feature —
  launch the workstreams, write and run the probes, verify staging, triage the
  local review, run the fix lanes, decide production. Use when a `plan.md` exists and has passed
  the path checker: "/feature:orchestrate <plan.md>", "run this plan", "ship this
  plan". NOT for planning, and not for a change small enough to do in one
  turn — there the launch overhead is the whole cost.
---

# Orchestrate — run the plan, never write the code

The orchestrator is the one seat that sees both lanes. It launches, verifies and decides; it does
not implement, and it does not drive UI. Every hour it spends editing the branch is an hour the
backend lane cannot merge onto it, and every UI step it drives by hand is a step a probe would have
answered in one background call.

Run this on Opus 4.8 medium (`/model claude-opus-4-8`, `/effort medium`) in a fresh session, reading `plan.md` and its `problem.md` — a phase
boundary is a task boundary, and the grounding sweep's stale tool output would cost reads without
helping (Complexity Trap, arXiv 2508.21433: masking it cut cost 52% at parity). Within the phase,
never `/clear` for size. Effort is set once at session start; escalate only for one hard problem,
then drop back.

## What plan.md must contain

The plan follows `${CLAUDE_PLUGIN_ROOT}/skills/orchestrate/plan-template.md`: Scope, Facts, Constraints,
Workstreams, UX workstream, Dependencies, Invariants, UX checklist per surface, New files, Checks,
Live checks, Outcome. Each workstream block is the implementer's whole brief, so `codex:execute`'s
spec.md is assembled from the plan's Constraints, Dependencies, Invariants and workstream blocks
without adding facts. The `planner` agent (`${CLAUDE_PLUGIN_ROOT}/agents/planner.md`, Fable high) writes it;
its brief carries these two lines verbatim:

> Repo facts come from `<path>/problem.md` only. If you need a fact that is not in it, stop and
> ask — do not infer it from naming, convention, or what a file like this usually contains.
>
> Scope is exactly <the ask>. Do not add steps that were not requested.

Before the plan ships, and after every revision, run `${CLAUDE_PLUGIN_ROOT}/skills/ground/check-paths.sh
<plan> [skip-regex]` from inside the repo. It exits 1 on any `MISSING:` or `AMBIGUOUS:` path;
mark files the plan creates `(new)` on their own line so they are skipped. Then grep each
function, route, table, column and env var the plan names — a miss there reads fluently, which is
why reading alone does not find it.

## Procedure

1. **Pin the base.** Read the plan. Compare `git rev-parse --short HEAD` against the SHA the plan
   records. If they differ, rebase onto the plan's base or re-check the plan's paths before
   launching — a plan is valid only at its SHA. Confirm the tree is clean.

2. **Launch the backend lane.** Invoke the `codex:execute` skill with the plan's codex workstreams
   (backend; frontend only when the plan has no UX lane). It runs in the background, verifies
   itself per workstream and post-merge on raw exit codes, merges onto the session branch and
   pushes a PR. It does not review; that is step 5. Do not spawn a
   verifier for its workstreams; confirm instead that its check command covers the touched surfaces.

3. **Launch the UX lane, in parallel.** Spawn the `ux-implementer` agent with the Agent tool, giving
   it the plan's UX workstream, the wire contract quoted as a real response body, and the UX
   checklist. It starts now, not after codex. It commits after every coherent step and returns flat
   JSON: `status` is `done`, `blocked` or `needs-backend`.

4. **While both run, write the probes.** Turn each line of the plan's UX checklist into a probe
   in the project's probe library, using its shared helpers (the project contract in the plugin
   README says where both live). This is the overlap the pipeline is built for. If no probe is
   needed, ground the next feature or poll the previous PR. NEVER edit the branch codex merges onto.

5. **Review and deploy staging, both on the push.** Watch the engine's output with `Monitor` for
   the line `pushed to origin` and act on it, not on the run's exit. First start the review in the
   background: `${CLAUDE_PLUGIN_ROOT}/../codex/skills/execute/review.sh <repo> <base> HEAD
   specs/<date>-<topic>/review.json <plan.md>`, where `<base>` is the engine's
   `.git/codex-execute/<feature>/pre-merge.sha`. A plan with no codex workstream has no engine
   run: start the same command when the UX lane reports `done`, with `<base>` the plan's base
   SHA. One review per plan, one round. Then run the project's staging deploy command, read the `sha` from its JSON, and record it as `STAGING_SHA`. Then run the project's
   staging verify command and read its verdict JSON. Both exit non-zero on failure; gate the next
   step on the exit code, not on the text. Then run the plan's Live checks against staging, a
   minute after the deploy returns — the first request after a deploy can still hit the old build. Only staging runs from
   here — production is the codex lane's command.

6. **Run the UX probes against staging.** Run them with `run_in_background` and read the verdict
   JSON. Failures go back to the UX agent by SendMessage to `ux-implementer` — it is idle, not
   dead, and keeps its context. At most two rounds; a finding that survives two rounds goes to the
   user.

7. **Triage, once.** When the review has written `review.json` (its process exits; a fail exit
   means no review, which is a finding for the user), drop every `nit`. Verify each `must-fix` against the code — severity is the reviewer's claim,
   not a fact — and add the probe failures from step 6 as findings of their own. Then decide the
   owner of each finding, exactly once: a finding in a backend module, an API route or a test
   file is `codex` without further thought (the project contract in the plugin README names the
   paths); for the rest ask one question, does the fix change what the user sees or does — `ux`
   if yes, `codex` if no. Findings in the same file get the same owner. Write them to
   `specs/<date>-<topic>/findings.json` as `[{file, line, claim, owner}]`. No findings means no
   fix round: go to step 9.

8. **Fix, both lanes at once, one round.** Give the UX lane its own tree so neither lane can
   dirty the other's: `git worktree add ../.ux-<branch> -b ux/<branch> origin/<branch>` then
   `cp -Rc <module>/node_modules ../.ux-<branch>/<module>/node_modules` for the module the UX
   findings touch (about 40 s; clone, never symlink — the codex sandbox writes through symlinks;
   add another module's the same way only for a finding there). Spawn `codex:codex-rescue` with
   the `codex` findings, told to work in the session checkout, commit, rebase onto the remote
   branch and push; spawn `ux-pr-fixer` with the `ux` findings, the worktree path, the side
   branch `ux/<branch>`, the PR branch and the UX checklist. A finding touching `.claude/**` or
   `CLAUDE.md` comes back for the user. There is no re-review: when both lanes are done, redeploy
   staging if the codex lane pushed, re-run only the probes for surfaces the fixes touched, then
   `git worktree remove --force ../.ux-<branch>` and `git branch -D ux/<branch>`, in the
   background (about 15 s). Post one PR comment summarising the findings and their dispositions;
   that comment is the review's record.

9. **Decide production, then record the outcome.** Production ships only when every finding is
   closed AND the re-run probes are green — never a half-shipped mixed feature. Merge the PR into
   the default branch; services that deploy from it on merge need nothing more, and the rest go
   through the codex lane's deploy command (the project contract in the plugin README says
   which). Then run the plan's Live checks against production. The
   phase's record is the PR, the deploy SHAs, the probe verdicts and the live-check results; write
   them into `plan.md` under its Outcome heading, write the memory files, and end. Write a separate
   `specs/<date>-<topic>/handoff.md` only if you must stop mid-phase (context past ~300k, quota
   exhausted), naming exactly where to resume.

## Hard rules

- **Every wait ends the turn.** Launch, say one line about what is running, and stop. Task
  notifications re-invoke you when a lane finishes. NEVER idle-wait, and NEVER call `TaskOutput`.
- **No `sleep` in the foreground.** Anything that waits runs with `run_in_background`.
- **No UI driving from the main loop.** Probes only, run with `run_in_background`, verdict JSON
  read back. Delegate to `ui-verifier` only what a probe cannot express: a freeform walk, or a step
  needing judgment in flight. Judge screenshots yourself; taste calls go to the user.
- **Inconclusive is not a pass.** Fix the probe or the environment and re-run. "Probe may be stale"
  means the screen model no longer matches the app: update the probe with the deliberate change, or
  treat the mismatch itself as the finding.
- **You do not edit the branch.** Not a typo fix, not a lint fix. Findings go to the agent that owns
  the file. The one exception is the probe scripts, which no lane owns. Creating and removing the
  UX lane's worktree is setup, not an edit.
- **Effort is not toggled**, and ad-hoc Agent spawns cannot set effort — that is why the UX lanes
  are defined agents. Do not spawn agents from a high-effort turn.
- **Agents are idle, not dead.** Send findings back by message and keep their context. Drop one only
  when its work is done or it has idled past the one-hour cache TTL, then spawn fresh with a short
  brief.
- **Loops are capped.** One fix round and no re-review; the re-run of the probes on the surfaces
  the fixes touched is the second gate. A finding that survives the round goes to the user.
  Review severity labels are unranked input; verify a finding before acting on it.
- **Memory at the phase end only** — written in step 9, not mid-turn; no handoff unless stopping mid-phase.
