# feature

Ship a feature from a written plan, with Claude orchestrating and never
implementing. `/feature:orchestrate` reads `plan.md`, launches the backend lane
through [codex](../codex/README.md)'s `/codex:execute`, launches the UX lane as
the `ux-implementer` agent in parallel, writes the UI probes while both run,
deploys and verifies staging, triages the execute run's local review once
into `findings.json`, fixes in two lanes (`codex-rescue` and the
`ux-pr-fixer` agent) with no re-review, re-probes the touched surfaces, and
decides production only when every finding is closed and the probes are
green. The
`planner` agent writes `plan.md` from a grounded `problem.md` following
`skills/orchestrate/plan-template.md`; `/feature:handoff` records a mid-phase
stop in under 40 lines.

The pipeline is `/feature:ground` → `planner` → `/feature:orchestrate` →
review-fix → deploy. Grounding takes its ask in the shape of the project's
`docs/user-template.md` (Ask, Example, Accept when, Constraints, Premises, Keep unchanged).

## Skills

| Skill | What it does |
|---|---|
| `/feature:orchestrate` | Run a `plan.md` through the codex and UX lanes to a shipped feature |
| `/feature:handoff` | Write a mid-phase handoff: stopped at, done, next, unverified, do not redo |
| `/feature:ground` | Pin repo facts into `problem.md` before planning; checks every path and anchor against the base commit |

## Agents

| Agent | Model | What it does |
|---|---|---|
| `planner` | Fable high | Write `plan.md` from `problem.md` using the plan template; repo facts from `problem.md` only |
| `ux-implementer` | Fable medium | Implement one UX workstream on the session branch, commit after every step, return flat JSON |
| `ux-pr-fixer` | Fable medium | Fix the PR threads labelled `claude-code-ux` in the UX lane's worktree, push, reply, resolve |
| `workstream-verifier` | Sonnet low | Rerun a delegated workstream's touched suites unpiped and return an objective verdict |
| `ui-verifier` | Sonnet low | Drive a scripted UI scenario (browse or iOS simulator) and return a verdict with evidence paths |

## Hooks

| Hook | Event | What it does |
|---|---|---|
| `subagent-no-spawn` | `SubagentStart` | Tells every subagent it cannot spawn agents and that background commands never wake it |
| `deny-foreground-poll` | `PreToolUse` Bash | Rewrites foreground waits (`until`/`while` loops, `tail -f`, `sleep` ≥ 10 s) to `run_in_background`; subagents exempt |
| `deny-blocking-taskoutput` | `PreToolUse` TaskOutput | Denies blocking `TaskOutput`; the task notification re-invokes the session instead |

## Project contract

The skills and agents are written against a project that provides the
following, described in its `AGENTS.md` (or `CLAUDE.md`) so every agent finds
them there:

- **A UI probe library and its shared helpers.** Scripted probes, one per
  surface, that print a verdict JSON and exit non-zero on failure; the
  orchestrator writes new probes with the shared helpers and runs them with
  `run_in_background`. Name the probe directories and the helper file.
- **A staging deploy command** that prints JSON containing the deployed `sha`
  and exits non-zero on failure, and **a staging verify command** that prints a
  verdict JSON and exits non-zero on failure. The orchestrator gates on exit
  codes, never on output text.
- **Backend paths**: the modules, API-route directories and test-file
  patterns whose findings always belong to the codex lane, so the orchestrator
  judges UX ownership only for the rest.
- **Which services deploy from the default branch on merge**, and **a
  production deploy command** owned by the codex lane for the rest; the
  orchestrator triggers it only through that lane.
- **Per-module test scripts** callable with a path filter (the plan's single
  `Checks` command chains them with `&&`), and **a dev-server command** that
  takes a port flag plus the default dev port to keep clear of, for the UX lane's
  probes.

For chadwallet these are `website/scripts/ui-probes/`,
`mobile/scripts/ui-probes/` and `scripts/ui-probes/lib.sh`;
`scripts/deploy-staging.sh` and `scripts/verify-staging.sh`; backend paths
`proxy/`, `streamer/`, `scraper/`, `website/src/app/api/` and any test file;
Render services and Vercel deploy from `main` on merge while the Cloudflare
Workers (proxy, streamer) still go through the codex lane's deploy command;
`yarn -s test` per module; `yarn dev -p <port>` with port 3004 reserved.

## Install

```
/plugin marketplace add blockchainian/claude
/plugin install feature@blockchainian
```

Requires the [codex](../codex/README.md) plugin for the backend lane and `jq`
for the hooks. If the same hooks are also wired in `~/.claude/settings.json`,
remove them there; otherwise each fires twice.

## Test

```
npm run test:feature
```

Runs `hooks/tests/run.sh`: every case in `hooks/tests/cases.jsonl` through the
three hook scripts.

## License

MIT
