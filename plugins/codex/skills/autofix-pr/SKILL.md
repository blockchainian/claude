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
  [--repo DIR] [--max-rounds 2] [--production] [--ux-file PATH] \
  [--wait 900] [--poll 10] [--timeout 3600]
```

Launch it in the background and END YOUR TURN. Each round:

1. Waits for a review submitted on the current head commit, or a review comment or reaction
   on the PR newer than that commit's time, from anyone but the account running the engine.
   The head is the branch ref's commit, not the pull request's head, which GitHub can leave
   lagging a push by minutes
   — one GraphQL query per poll, at most `--wait / --poll` checks. GraphQL has its own rate budget,
   so several engines polling at once leave the REST budget to the Codex skill. A clean Codex re-review leaves
   no review: the bot reacts with a thumbs-up on the PR, so thumbs-up reactions count. Its
   "eyes" reaction only means a review is in progress. The Codex skill's own thread
   replies, which GitHub wraps in reviews, do not count. No review means no round:
   the run stops, reports the state it read, and sets `awaiting_review`.
2. Reads every review thread over GraphQL and classifies the unresolved ones.
3. Runs the Codex fix-pr skill as a daemon thread named `<pr>/fix-pr r<n>`, visible in
   `codex agents` with its live status, then re-reads the PR head. With `--ux-file`, the
   threads are re-read every poll while the skill works, and each thread it routes to
   Claude Code is appended to that file the moment it is marked, so the UX lane can start
   in parallel instead of after the round. The skill is told that a finding is Claude Code's only when its fix lands under
   `website/src/components/**`, `website/src/app/**`, `mobile/app/**` or
   `mobile/src/components/**` AND changes what the user sees or does (label and mark, do not
   fix); logic fixes in those paths that leave the rendered result unchanged stay with Codex.
   It is also told that marked threads belong to Claude Code, and to rebase onto the remote PR
   branch before every push.

After a push, the next iteration waits for the reviewer to react to the new head before
reading its threads, so a head the reviewer has not seen is never reported as clean. The
run ends when a reviewed head has no unresolved must-fix thread, after `--max-rounds`, or
when no review arrives in time.

By default the Codex skill stops after staging verification and reports the staging SHA;
the production flip stays with the caller. `--production` lets it deploy production
itself on the first clean round.

## Output

One flat JSON object on stdout (progress goes to stderr):

```
{"rounds": 1, "awaiting_review": false, "pushed": ["<sha>"], "staging_deploys": ["<sha>"],
 "ux_threads": ["<thread id>"], "config_threads": ["<thread id>"], "remaining": []}
```

- `awaiting_review` — true when the run stopped because no review newer than the PR head
  arrived within `--wait`; the thread lists then describe an unreviewed head.
- `pushed` — PR head SHAs observed after a round, in order.
- `staging_deploys` — the `sha` of every `deploy-staging.sh` JSON result the Codex skill
  quoted in its report.
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
