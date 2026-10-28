# web tests slower than 1s

From one full `npm test` run on 2026-10-27, branch `fix-slow-tests`. Node tests are timed per test; shell and Python test scripts are timed per whole file.

## browse

| # | Seconds | Test | File |
|---:|---:|---|---|
| 1 | 23.54 | build.sh fetches gstack at GSTACK_COMMIT, patches it, and the patches' own tests pass | plugins/web/skills/browse/tests/patches.test.mjs |
| 2 | 5.99 | the first call builds into CLAUDE_PLUGIN_DATA and a second call reuses that build | plugins/web/skills/browse/tests/launcher.test.mjs |
| 3 | 4.09 | two concurrent first calls build once | plugins/web/skills/browse/tests/launcher.test.mjs |
| 4 | 1.05 | develop.sh commits every patch as the baseline of a fresh worktree | plugins/web/skills/browse/tests/patches.test.mjs |
| 5 | 1.01 | check-patches.sh applies every patch in order and removes its worktree | plugins/web/skills/browse/tests/patches.test.mjs |

## check-web-design

| # | Seconds | Test | File |
|---:|---:|---|---|
| 1 | 6.28 |  | plugins/web/skills/check-web-design/tests/test_check_design.py |
