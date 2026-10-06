# codex

A community extension of the [official codex plugin for Claude Code](https://github.com/openai/codex-plugin-cc)
that lets Claude run codex threads on the shared local app-server daemon as
supervised workers. It ships under the same `codex` plugin name and adds one
MCP server, `codex-manager`: Claude starts a thread, gives it follow-up
prompts, answers its questions and approval requests, interrupts it, lists
what is running, asks it for a code review, and gets woken when it finishes
or has something to say. Codex reaches Claude through two tools,
`notify_claude` and `ask_claude`, served by an MCP server this plugin ships.
Threads Claude starts get that server with the thread; sessions Claude
attaches to need it in codex's own config (see Install).

The [feature](../feature/README.md) plugin's `/feature:ship` uses it as the
backend lane: one thread per workstream, each in its own worktree, with
Claude merging behind the plan's check command.

## Install

```
/plugin marketplace add blockchainian/claude
/plugin install codex@blockchainian
```

`blockchainian` is the marketplace declared by this repository, which also
ships [feature](../feature/README.md).

This plugin intentionally shares the `codex` plugin name with the official
plugin. Installing both is supported — installed-plugin identity is
`name@marketplace`, so they coexist as `codex@openai-codex` and
`codex@blockchainian` — but the shared `/codex:` command namespace is not
formally documented behavior; if the combination misbehaves in your setup,
please open an issue.

To let sessions you opened yourself talk back to the Claude session that
attached them, add the server to `~/.codex/config.toml` once. The marketplace
clone is the path that stays put across plugin updates:

```toml
[mcp_servers.claude]
command = "node"
args = ["/Users/you/.claude/plugins/marketplaces/blockchainian/plugins/codex/codex-manager/manager.mjs", "claude"]
tool_timeout_sec = 360
default_tools_approval_mode = "approve"
```

`tool_timeout_sec` has to be longer than `CODEX_MANAGER_REPLY_TIMEOUT_SECONDS`, or codex
gives up on `ask_claude` before the manager's own answer arrives. A session
picks the server up when it is started or resumed, so one that was already
open has to be reopened. In a session no Claude supervises, both tools fail at
once and say so.

Requirements: [codex CLI](https://github.com/openai/codex) ≥ 0.144, logged in,
with its app-server daemon running (`codex agents` starts one).

## Tools

| Tool | Does |
|---|---|
| `start(cwd, prompt, name?)` | `thread/start` on the daemon (approval `on-request`, sandbox `workspace-write` with network access, the `claude` MCP server in its config), name it, `turn/start` with the prompt, return immediately with the thread id and the await command. |
| `attach(thread)` | Take over a session that is already running elsewhere (the codex TUI, say), found by its thread id or exact name. `thread/resume` with nothing but the id subscribes to it, so its settings stay as its own client set them. After that `send`, `reply`, `interrupt`, `list` and the await command work on it. |
| `send(threadId, prompt)` | `turn/start` on an existing thread. When a turn is still running the prompt is injected into it (the daemon's start-or-steer rule); the result says which happened. |
| `reply(threadId, text, callId?)` | Answer what the thread is waiting for: the text becomes the result of its `ask_claude` call, or, for an approval request, one of the decisions its inbox event listed. `callId` picks one when several are waiting. |
| `interrupt(threadId)` | `turn/interrupt` the thread's current turn. |
| `list()` | The threads this Claude session started or attached, their last turn status, and the unread inbox count per thread. |
| `detach(threadId)` | `thread/unsubscribe` a thread whose work is finished, and keep it recorded as detached so a restart or reconnect does not subscribe to it again. The daemon unloads an idle thread 60 seconds (`thread_unload_delay_secs`) after its last subscriber leaves, which moves it from Ready to Inactive in `codex agents`. Refused while a turn is running. A later `send` or `attach` subscribes again. |
| `review(cwd, base, out, plan?, decisions?, name?, stance?, focus?)` | A read-only thread running codex's own review mode (`review/start`, its rubric and P0–P3 priorities) over the commits since `base`; the rendered review — an overall verdict and one finding per line with its file and lines — is saved to `out`. With `plan`, the reviewer reads the spec first and does not flag a behaviour change the spec asks for. With `decisions`, the reviewer also reads the rules decided during the run: it does not report one as a departure from the spec, and judges each on its own, tracing what reads the result, also in code the changes did not touch. `stance: "adversarial"` adds a challenge stance (`codex-manager/adversarial-review.md`): the reviewer looks for the strongest reasons the change should not ship and questions the approach itself. `focus` names what it should weigh most. |

## Attached sessions

`attach` pages through `thread/list` and matches the name itself, because the
daemon's `searchTerm` filters titles. Sessions keep their name after they are
closed, so when several share one, the only one that is open is taken; with
none or several open, the name is refused with the candidates' ids.

The daemon sends a thread's approval requests to every client subscribed to
it, and the first answer settles them for all. So for an attached session the
manager answers none of the daemon's requests, not even with an error: they
stay with the client the session runs in. `notify_claude` and `ask_claude` do
reach Claude, because they do not travel through the daemon's requests.

## Getting woken

The MCP initialize response supplies concise lifecycle instructions even before
tools are loaded: start a thread, run its await command in the background,
answer asks with `reply` after checking `list` for `waiting` (an await can
deliver an already answered ask). A thread is finished only when its turn
completed (not failed, interrupted or waiting on an ask), its result has been
checked and accepted, and no further message is planned. Blocked threads and
results still being checked are not finished. Once finished, `detach` in the
same turn; a later `send` re-attaches it, so early detachment is safe. Before
ending a multi-thread run, `list` and detach every finished thread.

Every `start`/`send`/`review` result carries an await command. Run it with
`run_in_background`:

```
node <plugin>/codex-manager/manager.mjs await --thread <id> [--timeout <s>]
```

It prints the next unread inbox events as JSON lines and exits 0, which wakes
Claude. Four kinds of event arrive:

- `{"kind":"completed","status":"completed|interrupted|failed","lastMessage":…}`
  from `turn/completed`. A codex turn is a whole task, so this fires once per
  prompt, never per tool call. For a review thread `lastMessage` names the
  file the review was written to.
- `{"kind":"notify","text":…}` when codex calls `notify_claude`. The tools
  server appends the text to the inbox and answers at once; codex keeps
  working.
- `{"kind":"ask","callId":…,"text":…}` when codex calls `ask_claude`. The tools
  server keeps the call open, so codex blocks inside the same turn until
  Claude calls `reply`; the reply text is what codex receives as the tool's
  result.
- `{"kind":"approval","callId":…,"text":…,"decisions":[…],…}` when codex asks
  to run a command or change files outside its sandbox
  (`item/commandExecution/requestApproval`, `item/fileChange/requestApproval`).
  Claude answers with `reply` and one of the listed decisions.

An ask or approval that Claude does not answer within
`CODEX_MANAGER_REPLY_TIMEOUT_SECONDS` seconds (default 300) is answered for it: asks get
"proceed on your own judgment", approvals get `decline`. While something is
waiting, every tool result carries an `attention` list and the Stop hook
nudges once per stop. When the turn ends or a new one starts the daemon aborts
whatever was still held, and the manager withdraws the open questions too.

## How a tool call finds its Claude session

Codex starts the tools server itself (`manager.mjs claude`), so it
knows nothing of Claude's session. Codex sends the calling thread's id with
every MCP tool call (`_meta.threadId`). The manager records, for each thread it
starts, attaches or adopts, which session supervises it and the manager's pid;
the last session to claim a thread has it. The tools server looks the thread up
there, refuses when there is no record or that manager is no longer running,
and otherwise writes into that session's directory: the inbox event, and for
`ask_claude` the open question, which `reply` answers with a file the tools
server is polling for. `state.json` stays the manager's alone to write.

`--timeout` exits 124 with no output when nothing arrives. The plugin's Stop
hook (`codex-manager pending`) is the fallback: when Claude tries to stop with
undelivered events, the hook blocks once and hands them over. `await` and the
hook take events under a per-thread lock, so an event is delivered once even
when both are reading.

Only an `await` process wakes Claude after it has stopped, so the hook also
blocks while a thread's turn is running and no `await` is waiting on it, and
names the command to run. It does so on every stop, until one is running.

If the daemon connection drops (a daemon restart, say) the MCP server
reconnects with backoff while it has threads, `thread/resume`s them, and the
daemon replays any request that was still waiting for an answer.

## State

```
~/.claude/codex-manager/
  threads/<codex-thread-id>.json   the supervising Claude session and its manager's pid
  <claude-session-id>/
    state.json                          threads: id, name, cwd, turnId, lastStatus, waiting (held approvals), attached, review (out file)
    <codex-thread-id>.jsonl             the inbox, appended by the manager and the tools server
    <codex-thread-id>.cursor            byte offset of delivered events, written by await/pending only
    <codex-thread-id>.await.<pid>       an await process waiting on the thread, removed when it exits
    <codex-thread-id>.ask.<call>.json   a question codex is waiting on, written by the tools server
    <codex-thread-id>.reply.<call>.json its answer, written by reply and taken by the tools server
```

The session id comes from `CLAUDE_CODE_SESSION_ID` when set, else from the
first ancestor process that has a record in `~/.claude/sessions/` (Claude
Code registers the pid it was launched as, which may be a shell wrapper).
`/clear` gives the session another id while the MCP server keeps running with
the one it started with; on each tool call the server reads the id in that
record and, when it differs, links `<current-id>/` to its own directory, so the
await command and the Stop hook read the same inboxes. Because `claude
--resume` keeps the id, a resumed session reconnects to its threads: the MCP
server `thread/resume`s each recorded thread (which subscribes it to that
thread's notifications and replays any pending approval) and backfills a
`completed` event for turns that finished while Claude was away. Codex threads
outlive the Claude session: a running turn finishes on its own, and the daemon
unloads an idle, unsubscribed thread after `thread_unload_delay_secs`.

The daemon is reached over `~/.codex/app-server-control/app-server-control.sock`
(override with `CODEX_MANAGER_SOCKET_FILE`; state root with
`CODEX_MANAGER_STATE_DIR`).

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

Rejected: dynamic tools declared on `thread/start` for `notify_claude` and
`ask_claude` (the daemon routes their calls to the client that started the
thread, but they can be declared nowhere else, so a session Claude did not
start can never have them); a Claude subagent wrapping a codex CLI call per workstream (stacks
both latencies and cleans worktrees up under running jobs); `codex exec
review --base` (refuses custom instructions, so the plan cannot be the spec);
a hand-written review prompt with an output schema (a second rubric to keep
in step with codex's).

## Tests

```
npm run test:codex
```

`tests/codex-manager.test.mjs` drives the MCP server and the `await`/`pending`
readers against a fake daemon (`tests/helpers/fake-daemon.mjs`): thread start
parameters, approval forwarding, completion delivery, the review tool, adoption on
restart, reconnect after a dropped daemon connection, daemon-down error
reporting, and attaching to a session by name or id. The tools server runs as
a second child on the same state directory, called the way codex calls it:
`notify_claude`, `ask_claude` answered by `reply`, ask timeout, cancelled and
abandoned calls, and threads nobody supervises.

## License

MIT

## Environment Variables

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `CODEX_MANAGER_STATE_DIR` | Manager state; default ~/.claude/codex-manager | Optional | Shell environment |
| `CODEX_MANAGER_SOCKET_FILE` | Daemon Unix socket path | Optional | Shell environment |
| `CODEX_MANAGER_SESSIONS_DIR` | Claude sessions; default ~/.claude/sessions | Optional | Shell environment |
| `CODEX_MANAGER_REPLY_TIMEOUT_SECONDS` | Supervisor reply timeout; default 300 seconds | Optional | Shell environment |

`CODEX_HOME` and host-provided `CLAUDE_CODE_SESSION_ID` retain their upstream names.
