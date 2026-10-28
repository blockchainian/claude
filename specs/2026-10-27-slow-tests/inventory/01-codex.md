# codex tests slower than 1s

From one full `npm test` run on 2026-10-27, branch `fix-slow-tests`. Node tests are timed per test; shell and Python test scripts are timed per whole file.

| # | Seconds | Test | File |
|---:|---:|---|---|
| 1 | 10.11 | detach CLI fails after ten seconds when the turn stays running | plugins/codex/tests/detach-cli.test.mjs |
| 2 | 10.08 | detach CLI fails after ten seconds when the turn stays unanswered | plugins/codex/tests/detach-cli.test.mjs |
| 3 | 3.40 | pending blocks while a running thread has no await, however often Claude stops | plugins/codex/tests/codex-manager.test.mjs |
| 4 | 3.13 | reconnect keeps retrying while the daemon is down and does not redeliver a known completion | plugins/codex/tests/codex-manager.test.mjs |
| 5 | 1.69 | await prints unread lines, advances the cursor, and times out on silence | plugins/codex/tests/codex-manager.test.mjs |
| 6 | 1.44 | a detached thread is not subscribed to again when the server starts or reconnects | plugins/codex/tests/codex-manager.test.mjs |
| 7 | 1.23 | mcp reconnects after the daemon drops the connection and resumes its threads | plugins/codex/tests/codex-manager.test.mjs |
| 8 | 1.19 | an unanswered ask times out with a proceed-on-your-own answer | plugins/codex/tests/codex-manager.test.mjs |
