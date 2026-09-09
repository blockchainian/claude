---
name: autofix-pr
description: Work a pull request's review threads to done through bounded rounds of the Codex fix-pr skill — wait for review feedback newer than the PR head, run codex as a daemon thread, re-read the PR, and report the pushes, staging deploys, UI threads routed back to Claude Code and threads left for the user. Use when a PR is under review and its backend findings should be fixed off the Claude critical path.
---

# codex:autofix-pr — work a PR's review threads off the Claude critical path

Codex owns the fixing; Claude owns the routing. Engine:
`${CLAUDE_PLUGIN_ROOT}/skills/autofix-pr/autofix-pr.sh`. The Codex-side skill it invokes lives at
`~/.codex/skills/fix-pr` and already triages findings, fixes must-fix bugs, replies and
resolves threads, and deploys the affected backend services.

## When to use

After `codex:execute` (or any lane) has opened a PR and a reviewer — the GitHub Codex app,
`/code-review`, or a person — has left findings. NOT for the first implementation pass, and
NOT for UI work: UI-changing findings come back for Claude Code to implement.

## Run it

```
${CLAUDE_PLUGIN_ROOT}/skills/autofix-pr/autofix-pr.sh --pr <number> \
  [--repo DIR] [--max-rounds 2] [--staging-only] \
  [--wait 1800] [--poll 30] [--timeout 3600]
```

Launch it in the background and END YOUR TURN. Each round:

1. Waits for a review comment created after the current PR head's commit time — a bounded
   `gh api` poll, at most `--wait / --poll` checks. No new review means no round: the run
   stops and reports the state it read.
2. Runs the Codex fix-pr skill as a daemon thread named `<pr>/fix-pr r<n>`, visible in
   `codex agents` with its live status.
3. Re-reads the PR: head SHA, labels, and every review thread over GraphQL.

The run ends when no unresolved must-fix thread remains, or after `--max-rounds`.

`--staging-only` tells the Codex skill to stop after staging verification and report the
staging SHA, leaving the production flip to the caller. Without it the Codex skill ships
production itself on the first clean round.

## Output

One flat JSON object on stdout (progress goes to stderr):

```
{"rounds": 1, "pushed": ["<sha>"], "staging_deploys": ["<sha>"],
 "ux_threads": ["<thread id>"], "config_threads": ["<thread id>"], "remaining": []}
```

- `pushed` — PR head SHAs observed after a round, in order.
- `staging_deploys` — commit-shaped tokens the Codex skill reported on a line mentioning
  staging.
- `ux_threads` — unresolved threads the Codex skill routed to Claude Code: the PR carries
  the `claude-code-ux` label and the thread carries its `[UX — Claude Code]` reply. Hand
  these to the `ux-pr-fixer` agent.
- `config_threads` — unresolved threads whose path or text names `.claude/**` or
  `CLAUDE.md`. The Codex skill never edits Claude Code configuration, so these are the
  user's call.
- `remaining` — unresolved threads that are neither, i.e. must-fix work still open.

Exit 0 = `remaining` empty; 2 = threads remain; 1 = fatal (no PR, no daemon, no `gh`).

Neither UX nor config threads block completion — they are routed, not failed.

## Tests

`skills/autofix-pr/tests/test-autofix-pr.sh` stubs `gh` and the daemon runner through `PATH` and
`FIXPR_DAEMON_RUNNER`; it never touches a real PR or a real Codex.
