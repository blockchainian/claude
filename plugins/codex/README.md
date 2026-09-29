# codex

A community extension of the [official codex plugin for Claude Code](https://github.com/openai/codex-plugin-cc)
that lets Claude run codex threads on the shared local app-server daemon as
supervised workers. It ships under the same `codex` plugin name and adds one
MCP server, `codex-manager`: Claude starts a thread, gives it follow-up
prompts, answers its questions and approval requests, interrupts it, lists
what is running, asks it for a code review, and gets woken when it finishes
or has something to say. Nothing is installed on the codex side; codex sees
two extra tools declared per thread, `notify_claude` and `ask_claude`.

The [feature](../feature/README.md) plugin's `/feature:ship` uses it as the
backend lane: one thread per workstream, each in its own worktree, with
Claude merging behind the plan's check command.

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

Requirements: [codex CLI](https://github.com/openai/codex) ≥ 0.144, logged in,
with its app-server daemon running (`codex agents` starts one).

## Tools

| Tool | Does |
|---|---|
| `start(cwd, prompt, name?)` | `thread/start` on the daemon (approval `on-request`, sandbox `workspace-write` with network access), name it, `turn/start` with the prompt, return immediately with the thread id and the await command. |
| `send(threadId, prompt)` | `turn/start` on an existing thread. When a turn is still running the prompt is injected into it (the daemon's start-or-steer rule); the result says which happened. |
| `reply(threadId, text, callId?)` | Answer what the thread is waiting for: the text becomes the result of its `ask_claude` call, or, for an approval request, one of the decisions its inbox event listed. `callId` picks one when several are waiting. |
| `interrupt(threadId)` | `turn/interrupt` the thread's current turn. |
| `list()` | The threads this Claude session started, their last turn status, and the unread inbox count per thread. |
| `review(cwd, base, out, plan?, decisions?, name?, stance?, focus?)` | A read-only thread running codex's own review mode (`review/start`, its rubric and P0–P3 priorities) over the commits since `base`; the rendered review — an overall verdict and one finding per line with its file and lines — is saved to `out`. With `plan`, the reviewer reads the spec first and does not flag a behaviour change the spec asks for. With `decisions`, the reviewer also reads the rules decided during the run: it does not report one as a departure from the spec, and still reports one that is itself a defect. `stance: "adversarial"` adds a challenge stance (`codex-manager/adversarial-review.md`): the reviewer looks for the strongest reasons the change should not ship and questions the approach itself. `focus` names what it should weigh most. |

## Getting woken

Every `start`/`send`/`review` result carries an await command. Run it with
`run_in_background`:

```
node <plugin>/codex-manager/codex-manager.mjs await --thread <id> [--timeout <s>]
```

It prints the next unread inbox events as JSON lines and exits 0, which wakes
Claude. Four kinds of event arrive:

- `{"kind":"completed","status":"completed|interrupted|failed","lastMessage":…}`
  from `turn/completed`. A codex turn is a whole task, so this fires once per
  prompt, never per tool call. For a review thread `lastMessage` names the
  file the review was written to.
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

## State

```
~/.claude/codex-manager/<claude-session-id>/
  state.json            threads: id, name, cwd, turnId, lastStatus, waiting (held asks/approvals), review (out file)
  <codex-thread-id>.jsonl   the inbox, appended by the MCP server
  <codex-thread-id>.cursor  byte offset of delivered events, written by await/pending only
```

The session id comes from `CLAUDE_CODE_SESSION_ID` when set, else from the
first ancestor process that has a record in `~/.claude/sessions/` (Claude
Code registers the pid it was launched as, which may be a shell wrapper). Because `claude
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

## Design

Claude's context is the expensive resource: every worker transcript it reads
and every poll it makes grows it and slows the loop. So the worker runs on the
daemon, Claude ends its turn, and only three things come back: a completion,
a note codex chose to send, or a question codex needs answered. A codex turn is
a whole task, so a workstream costs Claude one wake-up plus whatever codex
asks. Coordination between workers goes through Claude, which holds the plan
and every worker's state; workers never message each other.

Review uses codex's built-in review mode rather than a prompt of its own: the
rubric, priorities and output shape are codex's, kept current upstream, and
the rendered text is what the daemon hands to every client. Severity is the
reviewer's claim — the caller verifies each finding against the code before
acting on it.

Rejected: a Claude subagent wrapping a codex CLI call per workstream (stacks
both latencies and cleans worktrees up under running jobs); `codex exec
review --base` (refuses custom instructions, so the plan cannot be the spec);
a hand-written review prompt with an output schema (a second rubric to keep
in step with codex's).

## Test

```
npm run test:codex
```

`tests/codex-manager.test.mjs` drives the MCP server and the `await`/`pending`
readers against a fake daemon (`tests/helpers/fake-daemon.mjs`): thread start
parameters, `notify_claude` relay, `ask_claude` hold and `reply`, ask timeout,
approval forwarding, completion delivery, the review tool, adoption on
restart, reconnect after a dropped daemon connection, and daemon-down error
reporting.

## License

MIT
