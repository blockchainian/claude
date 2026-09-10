---
name: orchestrate
description: >
  Run a written plan through the codex and UX lanes to a shipped feature —
  launch the workstreams, write and run the probes, verify staging, work the
  PR threads, decide production. Use when a `plan.md` exists and has passed
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
   (backend and non-UX frontend). It runs in the background, verifies itself per workstream and
   post-merge on raw exit codes, merges onto the session branch and pushes a PR. Do not spawn a
   verifier for its workstreams; confirm instead that its check command covers the touched surfaces.

3. **Launch the UX lane, in parallel.** Spawn the `ux-implementer` agent with the Agent tool, giving
   it the plan's UX workstream, the wire contract quoted as a real response body, and the UX
   checklist. It starts now, not after codex. It commits after every coherent step and returns flat
   JSON: `status` is `done`, `blocked` or `needs-backend`.

4. **While both run, write the probes.** Turn each line of the plan's UX checklist into a probe
   in the project's probe library, using its shared helpers (the project contract in the plugin
   README says where both live). This is the overlap the pipeline is built for. If no probe is
   needed, ground the next feature or poll the previous PR. NEVER edit the branch codex merges onto.

5. **Deploy and verify staging.** When codex reports done: run the project's staging deploy
   command, read the `sha` from its JSON, and record it as `STAGING_SHA`. Then run the project's
   staging verify command and read its verdict JSON. Both exit non-zero on failure; gate the next
   step on the exit code, not on the text. Then run the plan's Live checks against staging, a
   minute after the deploy returns — the first request after a deploy can still hit the old build. Only staging runs from
   here — production is the codex lane's command.

6. **Run the UX probes against staging.** Run them with `run_in_background` and read the verdict
   JSON. Failures go back to the UX agent by SendMessage to `ux-implementer` — it is idle, not
   dead, and keeps its context. At most two rounds; a finding that survives two rounds goes to the
   user.

7. **Work the PR, both lanes at once.** First give the UX lane its own tree so neither lane can
   dirty the other's: `git worktree add ../.ux-<pr> -b ux/<pr> origin/<branch>` then
   `cp -Rc <module>/node_modules ../.ux-<pr>/<module>/node_modules` for the module the UX
   threads touch (about 40 s; clone, never symlink — the codex sandbox writes through symlinks;
   add another module's the same way only if a thread there appears). Then launch `codex:autofix-pr` with `--ux-file <scratchpad>/ux-<pr>.txt`
   and never with `--production`: its default stops the Codex skill after staging, and
   production is step 9's decision. It fixes verified must-fix backend threads, redeploys
   staging, and labels UX threads `claude-code-ux` — and with `--ux-file` it appends each such
   thread id to the file the moment Codex marks it. Watch the file with `Monitor`; on the first id
   spawn `ux-pr-fixer` with the PR number, the worktree path, the side branch `ux/<pr>`, the PR
   branch, the ids and the UX checklist; send later ids to the same agent by message. On every
   SHA in the engine's `staging_deploys`, update `STAGING_SHA` and re-run the UX probes. An
   `awaiting_review: true` result means the reviewer has not reacted to the pushed head yet:
   relaunch it later instead of reading `remaining: []` as clean.

8. **Close the UX lane.** `ux-pr-fixer` fixes each marked thread in its worktree, rebases onto the
   PR branch, pushes, replies and resolves; a thread touching `.claude/**` or `CLAUDE.md` comes
   back as a finding for the user. Its pushes get their own bot review, which the engine matches
   by head SHA, so the lanes never misread each other's heads. Codex is told marked threads belong
   to Claude Code and to rebase before every push. When both lanes are done: `git worktree remove
   --force ../.ux-<pr>` and `git branch -D ux/<pr>`, in the background (about 15 s).

9. **Decide production, then record the outcome.** Production ships only when the backend threads
   are closed AND the UX probes are green — never a half-shipped mixed feature. Trigger it through
   the codex lane's deploy command, then run the plan's Live checks against production. The
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
- **Loops are capped.** One fix round, one re-review as a check on the fixes, a second fix only on
  verified must-fix findings, never a third. Review severity labels are unranked input; verify a
  finding before acting on it.
- **Memory at the phase end only** — written in step 9, not mid-turn; no handoff unless stopping mid-phase.
