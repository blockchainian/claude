# New task flow verification

Status: implementation and verification passed on `agents-panel`.

## Gates

| Command | Result |
|---|---|
| `cargo test` | 1448 passed, 0 failed; 18 unit tests and 78 terminal probes ignored |
| `cargo clippy --all-targets` | Passed; non-blocking warnings recorded in the log |
| `cargo build --release` | Passed |
| `git diff --check` | Passed |

Post-commit gate results are also recorded in `new-task-final-gates/post-commit-verdict.json`. Final logs: `new-task-final-gates/{tests,clippy,release,probe,remaining,attention}.log`; all six commands returned 0 in `new-task-final-gates/verdict.json`. Semantic verdicts in `new-task-probe/verdict.json`, `new-task-remaining/verdict.json`, and `new-task-attention/verdict.json` are all `pass`.

## Release binary in tmux

All walks run `target/release/agents` in dedicated tmux servers with temporary Git repositories and isolated Claude/Codex configuration. Enter is always sent separately. Assertions use `tmux capture-pane`; persistence and hand-off also verify `agents.json` and real session/job identifiers. Test-created jobs, servers, image files and repositories were cleaned up.

| Scenario | Result | Evidence |
|---|---|---|
| Name → repository → branch → prompt | Passed | `new-task-probe/01-wizard-repository.txt`, `02-wizard-branch.txt`, `03-wizard-prompt.txt` |
| Border branch change keeps prompt | Passed | `new-task-probe/04-border-branch.txt`, `05-side-branch-keeps-prompt.txt` |
| Filled draft changes repository; next Enter asks for its branch | Passed | `new-task-probe/07-repository-reset.txt`, `08-missing-branch.txt`, `09-branch-pick-no-create.txt` |
| Wizard browser/list/branch-editor Esc closes one layer | Passed | `new-task-probe/01a-browser-escape-confirm.txt`, `02a-branch-editor.txt`, `02b-branch-editor-escape.txt`, `02c-branch-reopened.txt` |
| Named title double-click, edit, Esc keeps original name | Passed | `new-task-probe/10-name-escape.txt` |
| Prompt Esc confirms deletion; cancellation preserves prompt | Passed | `new-task-probe/11-escape-delete-dialog.txt`, `12-delete-cancel.txt` |
| Leave/reselect restores prompt text and image | Passed | `new-task-probe/14-leave-draft.txt`, `15-restore-draft.txt` |
| Quit/restart restores draft, prompt and image | Passed | `new-task-probe/16-restart-restore.txt` |
| Confirm deletion removes row and `state.drafts` | Passed | `new-task-probe/17-restart-delete.txt`, `18-draft-deleted.txt` |
| Restart after cwd deletion reopens repository selection | Passed | `new-task-remaining/missing-cwd-error.txt`, `missing-cwd-browser.txt` |
| Restart after branch deletion reopens branch selection | Passed | `new-task-remaining/missing-branch-error.txt`, `missing-branch-selector.txt` |
| Model/effort border picks preserve the prompt and persist | Passed | `new-task-remaining/model-picked.txt`, `effort-picked.txt`; selected `claude-opus-5-5` and `medium` |
| Initial unnamed Esc abandons without confirmation | Passed | `new-task-remaining/unnamed-escape.txt` |
| Non-Git leaf cannot be selected; prompt Enter reopens browser | Passed | `new-task-remaining/nongit-rejected.txt`, `nongit-reopens-browser.txt` |
| Real worktree failure, branch correction, retry and session hand-off | Passed | `new-task-remaining/worktree-failure.txt`, `retry-branch-edited.txt`, `retry-session-handoff.txt`; `state.drafts` becomes empty |
| Official Claude creation and selected session hand-off | Passed | `new-task-remaining/real-session-handoff.txt`, `real-job-states.json`; `state.drafts` becomes empty |
| Named draft yields to NeedsInput, then returns with text and image | Passed | `new-task-attention/03-attention-jumped.txt`, `04-attention-returned.txt` |

Real creation and retry used official `claude --bg`; the user's Keychain OAuth access token was supplied only through the child environment, with no credential values printed. Final real session IDs: `57e338b0-05b2-4964-a19d-a27d3343718b`, `0179d5ee-22f4-4b0d-b91f-1b49110aa350`. Hand-off was verified; model response completion was not a requirement of these probes.

Attention replayed the recorded registry payload through the real filesystem watcher and provider using the live probe process identity. Its first HTTP-hook attempt could not bind the isolated dashboard's fixed question port because another dashboard owned it; the corrected registry walk passed without a product workaround. The separate attention walk also checks the image path and text after the jump and return.

## Existing terminal probes

Updated the existing wizard, branch, prompt and mouse probes. Seven real tmux probes passed: three prompt, one branch, three draft probes. Results are in `terminal-probes-final-terminal_prompt_.log`, `terminal-probes-final-terminal_branch_name_edits.log`, and `terminal-probes-final-terminal_drafts.log`.

The worktree probe now writes the current `config.worktree_root` / `state.pinned` shape instead of obsolete top-level keys. Its earlier escalated run left one empty worktree directory; `.git` was checked against that probe's deleted source repository and the owned artifact was removed.

## Test-first evidence and limits

- The Git-only change first failed three behavior tests: a confirmed non-Git draft incorrectly submitted, a non-Git directory result was accepted, and a loaded directory that lost Git remained confirmed (`/tmp/new-task-git-only-red.log`). All now pass.
- Confirmation after an attention jump, Enter on Cancel restoring focus, and Ctrl+Enter respecting the draft rule each failed before their fixes (`/tmp/new-task-confirmation-red.log`, `/tmp/new-task-control-enter-red.log`). All now pass.
- The original row/retention/error/Esc tests, deleted-main repair and actual title double-click also have behavioral-red evidence. The new-task snapshot failed before regeneration.
- Early persistence/control tests were written before implementation, but their initial runs failed at missing APIs during integration; they do not supply individual behavioral-red records for every original design Tests item. Some recovery guards were added after the initial broad implementation. This historical test-first evidence gap remains; no failing test was removed to make the gates pass.

## Design and ownership

Only Git working trees can create tasks. A non-Git default stays unconfirmed; folders are navigable but cannot be confirmed. Scenario 9 now counts a repository picked before naming as confirmed. Scenario 3 now returns to the prompt after the repository result; only prompt Enter creates. `AGENTS.md` and `new-task-flow-design-v3.md` carry these decisions.

Pre-existing changes in `src/providers/claude/log.rs`, `src/providers/codex/rollout.rs`, `src/ui/log.rs` and `src/ui/tool_detail.rs` were not edited or staged by this session. Shared-file commits from other sessions were preserved: `4fa9ece2` carries the app plumbing; `b25cb65c` and `58a9eb2e` carry portions of the rendering, selection and terminal-probe integration. Their history was not rewritten. Unrelated uncommitted changes remain in the worktree.

Implementation commits: `2e186518` (persistence/images), `d18048e3` (wizard/rows/tests). The latter also includes the concurrently added `Session.cache_expires_at` field and its draft initializer because these shared files were committed whole; the other session's provider and UI implementation was preserved outside this session's staged paths.
