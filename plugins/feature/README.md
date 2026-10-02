# feature

Ship a feature from a written plan, with Claude orchestrating and never
implementing. `/feature:ship` reads `plan.md`, opens a worktree per workstream
(`skills/ship/scripts/workstream.sh`), launches the codex lane as one thread per
workstream through [codex](../codex/README.md)'s `codex-manager` and the UX lane
as one `ux-implementer` agent per UX workstream, all at once, checks and
merges each workstream as it finishes behind the plan's Checks command, deploys
and runs the checks the plan names, runs codex's review mode once per plan and
triages it into `findings.json`, fixes in two lanes with no re-review, re-runs
the touched UX checks, and decides production only when every finding is
closed and the checks are green. `/feature:handoff` records a mid-phase stop in under 40
lines.

The plan is the user's: written in plan mode, by hand, or by any agent. Ship
reads it by section and requires only Workstreams (each block ending in a
`Files:` line), Dependencies and Checks; `skills/ship/minimal-plan-template.md` shows
the full shape. Two checkers gate it before launch: `check-paths.sh` (every path
exists in the repo) and `check-overlap.sh` (no file on two workstreams).
`/feature:retro` closes the loop: run in a fresh session on a finished session,
it ranks the biggest wastes by real token cost (joining the orchestrator
transcript to each subagent's recorded usage) and routes each fix back into
ship, the plan template and memory.

## Skills

| Skill | What it does |
|---|---|
| `/feature:ship` | Run a `plan.md` through the codex and UX lanes to a shipped feature |
| `/feature:handoff` | Write a mid-phase handoff: stopped at, done, next, unverified, do not redo |
| `/feature:retro` | Run in a fresh session on a finished session: rank the biggest wastes by real token cost, classify each (knowable-fact miss / topology deviation / plan defect), and propose fixes to ship, the plan template and memory |
| `ship/scripts/check-paths.sh` | Flags a path named in the plan that does not exist in the repo; `(new)` files are skipped |
| `ship/scripts/check-overlap.sh` | Flags a file listed on two workstreams' `Files:` lines; the orchestrator runs it beside the path checker |
| `ship/scripts/workstream.sh` | `open <id>` a worktree per workstream, `check <id> <cmd>` in it, `merge <id> <cmd>` onto the session branch behind the same check, `base` for the review |
| `retro/scripts/extract.mjs` | Objective retro evidence for a named session: spawn ledger + token-share-by-role, joining each spawn's `tool_use.id` to `subagents/<agent>.meta.json`, plus the codex lane joined from `~/.codex/sessions` |
| `retro/scripts/efficacy.mjs` | Best-effort efficacy analysis: joins the `retro.json` outcome records in `~/.claude/retros` to `fixes.jsonl` and reports whether each applied fix's waste recurs — near-deductive for mechanical gates, suggestive otherwise |

## Agents

| Agent | Model | What it does |
|---|---|---|
| `ux-implementer` | Fable medium | Implement one UX workstream in the checkout its brief names (the session tree or its own worktree), commit after every step, return flat JSON |
| `ux-autofixer` | Fable medium | Fix the PR threads labelled `claude-code-ux` in the UX lane's worktree, push, reply, resolve |
| `ux-verifier` | Sonnet low | Drive a scripted UI scenario (browse or iOS simulator) and return a verdict with evidence paths |

## Hooks

| Hook | Event | What it does |
|---|---|---|
| `subagent-no-spawn` | `SubagentStart` | Tells every subagent it cannot spawn agents and that background commands never wake it |
| `deny-foreground-poll` | `PreToolUse` Bash | Rewrites foreground waits (`until`/`while` loops, `tail -f`, `sleep` ≥ 10 s) to `run_in_background`; subagents exempt |
| `deny-blocking-taskoutput` | `PreToolUse` TaskOutput | Denies blocking `TaskOutput`; the task notification re-invokes the session instead |

## What the plan carries

Everything ship needs to know about the project is in the plan, so a repo
needs no contract file: the Checks command (before merge), an optional Deploy
section (`staging:` and `production:` commands printing JSON with the deployed
`sha`), optional UX checks (probe commands that print a verdict JSON), and
optional Deploy checks (commands run against each deploy). A CLI tool's plan
has Checks and nothing else; a web app's names its deploys and probes.

CI-watching needs nothing from the project: the plugin ships its own
GitHub/`gh`-based poller, `skills/ship/scripts/watch-ci.sh <ref> [out-file]`, used by
the production merge gate (step 8) whenever the branch has a PR.

## Install

```
/plugin marketplace add blockchainian/claude
/plugin install feature@blockchainian
```

Requires the [codex](../codex/README.md) plugin (its `codex-manager` MCP server) for the codex lane, `jq`
for the hooks and `uv` for `watch-ci.sh`. If the same hooks are also wired in `~/.claude/settings.json`,
remove them there; otherwise each fires twice.

## Tests

```
npm run test:feature
```

Runs `hooks/tests/hooks.test.sh` (every case in `hooks/tests/fixture/cases.jsonl` through the
three hook scripts), `skills/ship/tests/plan-checkers.test.sh` (the two plan checkers against
fixture plans in a throwaway repo), `skills/ship/tests/workstream.test.sh` (open, check,
merge and base against a throwaway repo), `skills/ship/tests/watch-ci.test.sh` (the CI-watch
poller against a stubbed `gh`), and `skills/retro/tests/retro.test.sh` (the retro
extractor against a hermetic fixture session).

## License

MIT
