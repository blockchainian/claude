# codex

A community extension of the [official codex plugin for Claude Code](https://github.com/openai/codex-plugin-cc)
that delivers a planned feature as **parallel codex workstreams, off Claude's
critical path**, and reviews the result into a findings file. It ships under
the same `codex` plugin name, so its two skills join the official plugin's
`/codex:` namespace as `/codex:implement` and `/codex:review`.

**The DX:** plan a feature with Claude Code (the [feature](../feature/README.md)
plugin's planner writes `plan.md`), then hand the plan to `/codex:implement`.
Claude writes `workstreams.txt`, one pointer line per workstream, and launches
`implement.sh` in the background. Codex implements the workstreams in parallel
worktrees, each gated by the plan's check command with bounded retries; green
workstream branches merge directly onto **the branch your session is on, in
your worktree** (codex resolves conflicts); a post-merge check gates delivery
and red restores your branch exactly to its pre-merge state; the branch is
pushed and its PR updated, or opened if none exists. `/codex:review` then
reviews the delta into a findings JSON for triage. Claude never polls, never
reads worker transcripts, and never re-enters the loop: when the run is done
the work is simply on your branch.

## Install

```
/plugin marketplace add blockchainian/claude
/plugin install codex@blockchainian
```

`blockchainian` is the marketplace declared by this repository, which also
ships [feature](../feature/README.md) and [grok](../grok/README.md).

This plugin intentionally shares the `codex` plugin name with the official
plugin. Installing both is supported — installed-plugin identity is
`name@marketplace`, so they coexist as `codex@openai-codex` and
`codex@blockchainian` — but the shared `/codex:` command namespace is not
formally documented behavior; if the combination misbehaves in your setup,
please open an issue.

Requirements: [codex CLI](https://github.com/openai/codex) ≥ 0.144 (logged
in), `git`, `timeout` (coreutils; `brew install coreutils` on macOS), and
optionally `gh` (authenticated) for automatic PR creation.

## Skills

| Skill | Responsibility |
|---|---|
| `/codex:implement` | Turn `plan.md` into `workstreams.txt`, launch `implement.sh` in the background, relay `summary.json` and the PR when it exits. |
| `/codex:review` | Run `review.sh` over a commit range and write `review.json` (`{findings: [{file, line, severity, claim}]}`, severity `must-fix` or `nit`) for the caller to triage. |

## codex-manager

The plugin also registers an MCP server, `codex-manager`, that lets Claude run
codex threads on the shared local app-server daemon as supervised workers:
start one, give it follow-up prompts, answer its questions and approval
requests, interrupt it, list what is running, and get woken when it finishes or
has something to say. Nothing is installed on the codex side; codex sees two
extra tools declared per thread, `notify_claude` and `ask_claude`.

### Tools

| Tool | Does |
|---|---|
| `start(cwd, prompt, name?)` | `thread/start` on the daemon (approval `on-request`, sandbox `workspace-write` with network access), name it, `turn/start` with the prompt, return immediately with the thread id and the await command. |
| `send(threadId, prompt)` | `turn/start` on an existing thread. When a turn is still running the prompt is injected into it (the daemon's start-or-steer rule); the result says which happened. |
| `reply(threadId, text, callId?)` | Answer what the thread is waiting for: the text becomes the result of its `ask_claude` call, or, for an approval request, one of the decisions its inbox event listed. `callId` picks one when several are waiting. |
| `interrupt(threadId)` | `turn/interrupt` the thread's current turn. |
| `list()` | The threads this Claude session started, their last turn status, and the unread inbox count per thread. |

### Getting woken

Every `start`/`send` result carries an await command. Run it with
`run_in_background`:

```
node <plugin>/codex-manager/codex-manager.mjs await --thread <id> [--timeout <s>]
```

It prints the next unread inbox events as JSON lines and exits 0, which wakes
Claude. Four kinds of event arrive:

- `{"kind":"completed","status":"completed|interrupted|failed","lastMessage":…}`
  from `turn/completed`. A codex turn is a whole task, so this fires once per
  prompt, never per tool call.
- `{"kind":"notify","text":…}` when codex calls `notify_claude`. The daemon
  routes that dynamic-tool call back to the MCP server, which answers it at
  once and appends the text to the inbox; codex keeps working.
- `{"kind":"ask","callId":…,"text":…}` when codex calls `ask_claude`. The MCP
  server holds the daemon's request open, so codex blocks inside the same turn
  until Claude calls `reply`; the reply text is what codex receives as the
  tool's result.
- `{"kind":"approval","callId":…,"text":…,"decisions":[…],…}` when codex asks
  to run a command or change files outside its sandbox
  (`item/commandExecution/requestApproval`, `item/fileChange/requestApproval`).
  Claude answers with `reply` and one of the listed decisions.

A held ask or approval that Claude does not answer within
`CODEX_MANAGER_ASK_TIMEOUT` seconds (default 300) is answered by the MCP
server: asks get "proceed on your own judgment", approvals get `decline`.
While something is held, every tool result carries an `attention` list and the
Stop hook nudges once per stop. When the turn ends or a new one starts the
daemon aborts whatever was still held, and the manager forgets it too.

`--timeout` exits 124 with no output when nothing arrives. The plugin's Stop
hook (`codex-manager pending`) is the fallback: when Claude tries to stop with
undelivered events, the hook blocks once and hands them over. `await` and the
hook take events under a per-thread lock, so an event is delivered once even
when both are reading.

If the daemon connection drops (a daemon restart, say) the MCP server
reconnects with backoff while it has threads, `thread/resume`s them, and the
daemon replays any request that was still waiting for an answer.

### State

```
~/.claude/codex-manager/<claude-session-id>/
  state.json            threads: id, name, cwd, turnId, lastStatus, waiting (held asks/approvals)
  <codex-thread-id>.jsonl   the inbox, appended by the MCP server
  <codex-thread-id>.cursor  byte offset of delivered events, written by await/pending only
```

The session id comes from `CLAUDE_CODE_SESSION_ID` when set, else from the
parent claude process's record in `~/.claude/sessions/`. Because `claude
--resume` keeps the id, a resumed session reconnects to its threads: the MCP
server `thread/resume`s each recorded thread (which subscribes it to that
thread's notifications and replays any pending tool call) and backfills a
`completed` event for turns that finished while Claude was away. Dynamic tools
are stored in the thread's rollout, so a resumed thread keeps them. Codex threads
outlive the Claude session: a running turn finishes on its own, and the daemon
unloads an idle, unsubscribed thread after `thread_unload_delay_secs`.

The daemon is reached over `~/.codex/app-server-control/app-server-control.sock`
(override with `CODEX_MANAGER_DAEMON_SOCKET`; state root with
`CODEX_MANAGER_HOME`). `dynamicTools` on `thread/start` is an experimental
app-server API, so the client initializes with `experimentalApi: true`.

`implement.sh` never reviews; the caller (the feature plugin's orchestrator, or
you when running standalone) runs `/codex:review` once per plan after
`implement.sh` reports `pushed to origin`.

## Use

In Claude Code, with a committed `plan.md` on the session branch:

```
/codex:implement
```

then, once `implement.sh` has pushed:

```
/codex:review
```

Both scripts also work standalone, no Claude required:

```
plugins/codex/skills/implement/implement.sh \
  --workstreams specs/my-feature/workstreams.txt --feature my-feature \
  --check "yarn test" --spec specs/my-feature/plan.md

plugins/codex/skills/review/review.sh . \
  "$(cat .git/codex-implement/my-feature/pre-merge.sha)" HEAD \
  specs/my-feature/review.json specs/my-feature/plan.md
```

### implement.sh

| Parameter | Meaning | Default |
|---|---|---|
| `--workstreams` | work list, one workstream per line (pointers into the spec) | required |
| `--feature` | run name; namespaces workstream branches and run state | required |
| `--check` | per-workstream verify command, run from the worktree root | required |
| `--base` | session branch to deliver onto; must match the checkout | checked-out branch |
| `--spec` | repo-relative path to the plan or spec, quoted in every prompt | – |
| `--concurrency` | worktree pool size | CPU count |
| `--retries` | per-workstream retry budget on red | 2 |
| `--timeout` | per-codex-invocation seconds (past it = red) | 2400 |
| `--setup` | run once per created worktree (deps provisioning) | – |
| `--deliver-wait` | seconds to wait for a clean session worktree at merge time | 1800 |
| `--runner` | `daemon` (rows in `codex agents`) or `exec` (standalone processes) | daemon |
| `--no-push` | stop after merge | off |

The session worktree must be clean when the run starts; results are delivered
by merging onto its branch at the end of the run. Each workstream commits to
its own branch, `workstreams/<feature>/<n>`. Red = check failed, codex
timeout, codex error, or no diff produced. Failed workstreams are excluded
from the merge and reported in the summary and PR body; their branches are
kept when they contain commits. Exit codes: `0` all green + delivered, `2`
partial (some workstreams failed; session branch green and delivered), `1`
post-merge check red (session branch restored to its pre-merge commit; green
workstream branches kept), merge blocked (worktree stayed dirty past
`--deliver-wait`, switched branch mid-run, or another run held the delivery
lock too long), or push failed. Delivery waits for a clean tree (untracked
files never delay it) and serializes across concurrent runs in the same repo
instead of aborting; the lock covers merge + post-merge check only, and a push
rejected by a moved remote is retried once after merging the remote tip in and
re-running the check. A workstream whose files collide with an untracked file
is excluded, its branch kept, without a codex conflict-resolution round.

Pre-merge HEAD is recorded both as the git ref
`refs/codex-implement/<feature>/pre-merge` and as
`.git/codex-implement/<feature>/pre-merge.sha`, which `/codex:review` takes as
its review base. The PR is opened from the session branch to the default
branch, which needs `gh`; it is skipped when the session is already on the
default branch.

Run state lives under `.git/codex-implement/<feature>/` (per-workstream status
JSON, logs, `pre-merge.sha`, `summary.json`); worktrees under
`../.codex-implement-<feature>/` exist only for the duration of the run.

### review.sh

`review.sh REPO BASE HEAD OUT_JSON [SPEC]`: a plain `codex exec` in a
read-only sandbox at high reasoning effort, with review instructions and
`review-schema.json`. It asks only for confident `must-fix` findings the change
itself introduced — broken behaviour, a violated invariant, an unhandled input
or error path, a security hole, a slowdown a user would notice — and no nits;
style and naming are not reported.
Codex's transcript lands in a `.log` beside the output. Exit 1 means no
review was produced. `REVIEW_TIMEOUT` (default 2400) bounds the run.

## Design

### Problem

"Many-at-once" backend work — a batch of independent workstreams — was slow
when Claude Code drove codex as a live orchestrator. Profiling put the root
cause on Claude sitting on the execution critical path, in two shapes:

1. **Model-driven orchestration.** The main loop decided each next step
   conversationally, so every hop was a full main-loop turn.
2. **A Claude subagent per workstream, shelling out to codex.** Each task paid
   Claude's latency plus codex's, stacked, N times, and the ephemeral agent
   worktrees were cleaned up under the still-running codex jobs.

Measured on the hybrid sessions: codex workstreams run 7–27 minutes each, and
the tax is the orchestrator staying resident around them — dispatch serialized
over 30 minutes of brief-writing, every workstream wakeup a full main-loop
turn (79 requests and 33 minutes of model wall during an 80-minute execution
phase), two mid-run compactions. Codex's parallelism never reached the wall
clock, and progress was invisible to the human, who ended up polling the
orchestrator. Taking Claude off the path halved wall-clock on that workload.

Latency facts behind the design: main-loop turns pay an effort-floor cost
before the first token (median ~4 s at high effort, ~7 s at xhigh, 13–27 s at
max); a large orchestrator context slows decode and forces mid-run
compactions, and it grows every time a worker's output is read back in.

### Core principle

**Take Claude off the execution path. git and the filesystem are the
coordination bus, not Claude's context.** Claude plans once, hands over once,
and never re-enters: execution, merge, push and review are codex + git +
script. Claude never ingests worker transcripts, so its context stays small
however many workstreams run.

### Pipeline

```
1. Handover  — Claude (one turn): workstreams.txt from plan.md, then launch
2. Execute   — codex ×N in parallel worktrees: workstream → check → branch (bounded retries)
3. Merge     — workstream branches → the session branch, in the session worktree; conflicts resolved there
4. Deliver   — post-merge check, push the session branch, update or open its PR
5. Review    — /codex:review over pre-merge..HEAD into review.json; the caller triages
```

**Handover is Claude's one turn.** Decomposition is the step that trades on
reasoning depth rather than speed, and workstream independence — which the
whole pipeline assumes — is produced by the planner, not checked at runtime.
Partition along file boundaries so no two workstreams edit the same file;
when overlap is unavoidable the plan's Dependencies records it so the merge
expects the conflict. `plan.md` is the spec codex reads; `workstreams.txt`
carries one pointer line per workstream, never the brief.

**Execute** keeps a worktree pool sized to concurrency, not to workstream
count: a workstream takes a free worktree, works, is checked and committed to
its own branch, and releases the worktree. Worktrees exist only from first
use to the end of merge. Red (check failed, timeout, codex error, no diff)
retries in the same worktree with the failure appended; exhaustion marks the
workstream FAILED and excludes it from merge. The default `daemon` runner
runs tasks through the shared codex app-server daemon so they show as rows in
`codex agents`; `exec` remains available for recovery.

**Merge** lands directly on the session branch. There is no intermediate
feature branch: results must end where the session is, and an integration
branch adds a merge stage plus a worktree needing deps. Pre-merge HEAD is
recorded, green branches merge one at a time, conflicts are resolved by codex
with the spec as context, and one post-merge check runs on the result since
conflict resolutions are code no workstream gate covered. Red restores the
branch with `git reset --keep` and keeps the green branches for autopsy.

**Deliver** pushes the session branch and updates its PR, or opens one to the
default branch. **Review** is a separate script the caller triggers, because
a plan with no codex workstream has no `implement.sh` run and still needs the review.
Correctness rests on deterministic gates — each workstream's check, the
post-merge check — plus one structured codex review whose severities are
labelled at the source, not on a single model's judgment used as a gate.

### Rejected alternatives (do not re-propose without new reasoning)

- **A Claude subagent wrapping codex per workstream** — double-wraps every
  workstream's latency and was the original slow design.
- **Claude review as the final gate** — the reliability of a single-model
  correctness review is weak, and a shallow review launders bad code as
  "reviewed". Deterministic checks plus a structured codex review instead.
- **`codex exec review --base` or a `@codex review` PR comment** — the former
  refuses custom instructions and a schema; the latter is slow, unobservable,
  and its threads need polling.
- **A shared worktree across workstreams** — data race on file writes.
- **One worktree per workstream** — the pool sized to concurrency suffices.
- **One PR per workstream** — fragments review into N diffs and never tests
  the integrated result; failed workstreams are already isolated at merge.
- **An intermediate feature branch for integration** — the run ends with the
  results somewhere the user is not, plus an extra merge stage and worktree.
- **A persistent worktree pool across runs** — standing cleanup discipline
  bought for a spin-up cost that is already small.
- **Codex as planner** — decomposition and conflict avoidance are the
  reasoning-depth step; that is the one Claude turn the pipeline keeps.

## Test

```
npm run test:codex
node --test tests/*.test.mjs
```

`tests/codex-manager.test.mjs` drives the MCP server and the `await`/`pending`
readers against a fake daemon (`tests/helpers/fake-daemon.mjs`): thread start
parameters, `notify_claude` relay, `ask_claude` hold and `reply`, ask timeout,
approval forwarding, completion delivery, adoption on restart, reconnect after
a dropped daemon connection, and daemon-down error reporting.

`skills/implement/tests/test-implement.sh` runs `implement.sh` against a fixture
repo with a stubbed codex CLI (pass, retry-with-failure-context, hang/timeout,
no-diff, merge-conflict resolution, session-branch delivery, restore-on-red,
delivery lock and wait, existing-PR update, pool bounds, cleanup, guard rails);
`skills/review/tests/test-review.sh` covers the review script's arguments,
sandbox and schema flags, prompt, output and failure exit against the same
stub. `implement.sh` is additionally verified against the real codex CLI.

## License

MIT
