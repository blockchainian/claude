# New task flow v3: wizard, side edits, persistent drafts

Wizard order: name → repository → branch → prompt. Tasks can only be created in Git working trees. The cheap choices come first; the prompt box is the last step and the only place a task is created from. A named draft is a stub task: listed with status `new`, persisted in `agents.json`, surviving leaving, attention jumps and restarts.

## Principle

A draft is a bag of fields. Two kinds of input touch it:

- **Wizard transitions** complete a step: Enter in the name editor while the draft has no name, a pick in the directory browser the wizard opened, a pick in a branch list the wizard opened, and Enter in the prompt box with text. After each, the next-step rule runs. Only prompt Enter can create the task.
- **Side edits** come from the details border (repository, branch, model, effort) and from double-clicking the name. They change one field, close their list, put focus back where it was, and never run the rule.

The rule reads only fields, never a stored position, so `Step` is removed. The prompt box content (text and images) is the prompt; there is no separately saved prompt. Enter with text runs the rule, which opens the first missing step or, when nothing is missing, creates the task.

## State model

| Field | Values | Set by | Counts as "set"? |
|---|---|---|---|
| `id` | `draft-<unix millis>-<counter>` string, assigned at N (no new crate) | `add_draft` | stable across runs; doubles as the row's `SessionKey.id`, distinct from provider session ids |
| `name` | `None`, `Some(text)` | name editor Enter | `Some`. Until then the draft is not listed and not persisted |
| `input` | prompt box text and image attachments | typing; stashed on leave, restored verbatim on select and after restart | the prompt; non-empty text is required at Enter |
| `setup.cwd` | always a path; defaults to the heading or process cwd | `add_draft`, repository change result | see `repository_confirmed` |
| `repository_confirmed` | `false`, `true` | `true` on any repository pick (wizard or border); `false` at N and when the directory is missing or no longer Git at load | the default directory is not confirmed |
| `setup.branch` | `""` (not a Git working tree) or the checked-out branch | derived by `draft_location` at N, on repository pick and at load; not persisted | informational |
| `branch` | `Unchosen`, `Existing(name)`, `New(name)` | branch list pick; reset to `Unchosen` by a repository change result whose `repository_identity(..).primary_root` differs from the previous one (a pick landing in the same repository, even from a subdirectory, keeps the choice: req 7), and at load when the branch no longer exists | `Unchosen` is not set; the checked-out default never counts until the user picks it, because the branch is the step that can create a worktree |
| `setup.model`, `setup.effort` | `Option<String>` | model and effort lists | optional |
| `continue_after_directory` | `bool` | set by a wizard repository pick, consumed by the change result | marks the one result that runs the rule |
| `working`, `job` | creation in flight / created | `create_task`, `draft_created` | internal freeze against a double submit; no visual state, the row stays `new` |
| `error` | `None`, `Some(message)` | `draft_created` Err, worktree Err, load repairs | shown in the details panel; cleared by the next wizard transition |

At creation `Existing(name)` equal to `setup.branch` maps to `branch: None` (run in the checkout, no worktree); any other `Existing` and `New` map to `tasks::BranchChoice`, so nothing downstream changes.

Each selector records `origin: Wizard | Border`. Only a pick in a `Wizard` list is a wizard transition. Focus has three homes: name editor, prompt box, selector. A side edit returns focus to the home it took it from: the name editor while unnamed, otherwise the prompt box.

## Next-step rule

Runs only after a wizard transition. First matching row wins. Clears `error` first.

| Condition | Next |
|---|---|
| `working` or `job` set | nothing |
| `name` is `None` | open the name editor, keep typed text, focus locked there |
| `repository_confirmed` is `false` or `setup.branch` is empty | keep the repository unconfirmed and open the directory browser with origin `Wizard` at the current repository's parent, current repository preselected |
| `branch` is `Unchosen` | open the branch list with origin `Wizard`, checked-out branch highlighted |
| the transition was prompt Enter | create the task |
| otherwise | focus the prompt box, typed text kept |

A wizard repository pick emits `ChangeDraftDirectory` with `continue_after_directory` set; `draft_location` in app.rs is synchronous and runs in the same dispatch, so the Ok result applies the new directory, resets `branch` if the root changed, clears the flag and runs the rule, landing in the branch list, or the prompt box when a same-root branch choice is already set. The Err result clears the flag, sets `error`, and focuses the prompt box. A border pick never sets the flag. A non-Git directory result is rejected without changing the directory or branch; plain folders can only be navigated, never confirmed.

## Task list

A named draft is a row. `set_sessions` appends `Session::from_draft` for every named draft: `id = SessionKey { provider, id: draft.id }`, `status: SessionStatus::New`, `title = name`, `cwd`, `git_branch` = chosen or checked-out branch, `prompt` = box text, `model`, `effort`, `updated_at` = last change, no process, no transcript. `View::draft_of(&SessionKey)` tells a draft row from a session row.

| Aspect | Rule |
|---|---|
| Sort | `SessionStatus::New` sorts after `NeedsInput` and before `Working`; among drafts by `session_order` (`updated_at` descending) |
| Filters | `Active` includes `New`; `Inactive` does not. `tab_after_refresh` treats `New` like `Ready` |
| Pinning, grouping, search | as any row |
| Attention (`reconcile_attention`) | the early return stays while the selected draft is unnamed (the name editor is modal); a named draft no longer blocks the jump and reappears intact when reselected |
| Row actions | delete only. `o`, `f`, archive, stop, rename shortcut, send prompt return `Action::Unsupported` |
| Row text | `new` badge, unchanged after Enter until the session arrives; `error` in the details panel |
| Unnamed draft | not a row; leaving it in any way drops it, as today |

A draft row is selected by key like any row: `selected` indexes it and `selected_id()` returns its key, so `set_sessions` re-finds it after every re-sort and pinning, tab memory, attention and the title double-click work unchanged. `selected_draft` is derived from `draft_of(selected_id())`; session-only paths gate on it. Selecting restores the stashed `input` into the prompt box and focuses it; the name editor and selectors are transient and never reopen on select. Leaving a named draft closes its selector, cancels an in-progress name edit (old name kept), stashes the box into `input`, and persists.

### Persistence

`agents.json` gains `state.drafts`, written through the locked `update` on every field change of a named draft (not on keystrokes), on leave and on quit (box content included). Unnamed drafts are never written.

```json
{
  "id": "draft-1760770000000-1",
  "provider": "claude",
  "name": "Fix login",
  "input": { "text": "half typed", "images": ["/var/folders/…/agents-image-x.png"] },
  "cwd": "/Users/me/Code/app",
  "repository_confirmed": true,
  "branch": "unchosen" | { "existing": "main" } | { "new": "feat/x" },
  "model": "opus", "effort": "high",
  "job": null,
  "error": null,
  "updated_at": "2026-10-18T09:00:00Z"
}
```

`StoredState.drafts` is a `Vec<serde_json::Value>` converted entry by entry, so a malformed draft drops alone with a notice instead of making `update()` replace the whole document with defaults (today an unparsable `StoredPreferences` wipes pinned, usage and worktree_root). Only the current shape is read, no compat code. Plumbing follows the existing patterns: `Preferences::save_drafts` writes only `state.drafts` like `save_pinned`; the view asks through `Action::PersistDrafts` like `PersistUsageView`; `App::new` hands loaded drafts to the view through `set_drafts` like `set_pinned`. The sessions watcher already ignores `agents.json`.

Images stay where they are: an attachment pasted into a named draft gets `ImageAttachment::preserve()` so its `TempPath` outlives the process, and the stored paths are its paths. At load, attachments are rebuilt from the stored paths; a missing file is dropped from the text, nothing more.

Load repairs, each recorded in `error`:

| Found at load | Repair |
|---|---|
| `cwd` missing | keep the draft; `repository_confirmed = false`, `branch = Unchosen`, `setup.branch = ""` |
| `Existing(x)` no longer a local branch | `branch = Unchosen` |
| checked-out branch changed | `setup.branch` re-derived; `branch` kept |
| `cwd` no longer a Git working tree | `repository_confirmed = false`, `branch = Unchosen`, `setup.branch = ""`; the rule opens the repository browser |
| `job` set, a session carrying that background job exists | draft removed |
| `job` set, no such session | `job = None`, `error = "Creation was interrupted"` |

### Creation hand-off

Prompt Enter with everything set calls `create_task`: `working` is set (freeze), the draft stays selected and its row stays `new`. `draft_created` Ok stores `job`; `reconcile_drafts` removes the draft from `rows` and `agents.json` only when the session carrying that job arrives, and selects that session. `draft_created` Err or a worktree Err clears `working`, sets `error`, persists, and leaves the draft editable with its prompt still in the box, so the user can change a field and press Enter again.

## Actions

| Control | State | Effect | Focus after | Creates? |
|---|---|---|---|---|
| **N** / double-click heading | any | new unnamed draft at that cwd, `repository_confirmed = false`, `branch = Unchosen`; name editor opens | name editor | no |
| Name editor Enter, non-empty | `name` `None` | sets `name`; the draft becomes a row and is persisted; wizard transition | per rule: directory browser | no |
| Name editor Enter, non-empty | `name` `Some` (double-click edit) | replaces `name`; side edit; persist | prompt box | no |
| Name editor Enter, empty | any | ignored | name editor | no |
| Double-click the title | `name` `Some` | opens the name editor prefilled (`name_target = id`, `rename.set_text`); today this path goes through `selected_row()` and does nothing for drafts | name editor | no |
| Prompt box Enter, text present | `name` `Some` | wizard transition | per rule: browser, branch list, or task created | yes, when repository confirmed and branch chosen |
| Prompt box Enter, box empty | any | ignored | prompt box | no |
| Border **repository** click | any | opens the directory browser, origin Border | selector | no |
| Repository pick | origin Wizard | `repository_confirmed = true`, `continue_after_directory = true`; emits `ChangeDraftDirectory` | the result decides | no; the result's rule opens the branch list or focuses the prompt box |
| Repository pick | origin Border | `repository_confirmed = true`; emits `ChangeDraftDirectory` | name editor if unnamed, else prompt box | no |
| Repository change result Ok | not frozen | sets `setup.cwd`, `setup.branch`; `branch = Unchosen` if the root changed; closes a branch list open for this draft; persists; runs the rule only if `continue_after_directory` | per rule, else unchanged | no |
| Repository change result Ok | frozen or gone | ignored | unchanged | no |
| Repository change result Err | any | `error`, flag cleared, fields unchanged | prompt box | no |
| Border **branch** click | Git repository | opens the branch list, origin Border, current choice or checked-out branch highlighted | selector | no |
| Border branch click | `setup.branch` `""` | no-op | unchanged | no |
| Branch pick | origin Border | sets `branch`, closes, persists | name editor if unnamed, else prompt box | no |
| Branch pick | origin Wizard | sets `branch`, persists; wizard transition | per rule: prompt box | no |
| Branch "create" row Enter / empty new name | any | starts typing / ignored | selector | no |
| Border **model** / **effort** click and pick | any | sets the value, closes, persists | name editor if unnamed, else prompt box | no |
| Any border click while another selector is open | any | replaces the selector; the new one is origin Border | selector | no |
| Backspace on a draft row | named, not frozen | opens the existing `ConfirmDialog` with the draft's name; the confirm sites branch on `draft_of(dialog.id)`: a draft is removed in the view and persisted, never emitted as `Action::Delete` (app.rs would answer "Nothing to delete") | previous row | no |
| Select another row, filter, attention jump | named | leaves the draft intact | the other row | no |
| Select another row, filter, attention jump | unnamed | drops the draft | the other row | no |
| Any control, Enter, Esc, delete | frozen | no-op | unchanged | no |
| Quit (Ctrl+C) | any | named drafts persisted with box content; unnamed dropped | n/a | no |

## Esc and cancel

Esc, Ctrl-C in a selector, and right-click undo one level: the innermost open thing closes. With nothing open on a named draft, Esc opens the delete confirmation, the same dialog Backspace opens, so discarding the whole task is one gesture from every state and always confirmed. On an unnamed draft Esc abandons it without asking.

| Where | Esc / right-click | State | Focus after |
|---|---|---|---|
| Name editor, `name` `None` | abandons the unnamed draft (fields picked earlier go with it) | draft removed | previous row |
| Name editor, `name` `Some` | discards the edit, keeps the old name | unchanged | prompt box |
| Prompt box, nothing open, named | opens the delete confirmation; confirm removes the draft, Esc in the dialog returns | unchanged until confirmed | prompt box, or previous row after delete |
| Directory browser, Wizard or Border | closes it; `repository_confirmed` unchanged; nothing created | unchanged | name editor if unnamed, else prompt box |
| Branch list, typing a new name | clears the typed name, stays in the list | unchanged | selector |
| Branch list, Wizard or Border | closes it; `branch` unchanged; nothing created; box content kept | unchanged | name editor if unnamed, else prompt box |
| Model / effort list | closes it | unchanged | same as above |
| Delete confirmation | cancels | unchanged | prompt box |
| Anything, frozen | no-op | unchanged | unchanged |

After Esc closes a wizard list the user is in the prompt box; the next Enter with text reopens the first missing step. No new hint states are added.

## Scenarios

| # | Steps | Result |
|---|---|---|
| 1 | N, name, Enter, Enter on the preselected repository, pick branch X, type prompt, Enter | task created on X |
| 2 | …, Enter on the highlighted checked-out branch, prompt, Enter | `Existing(checked-out)`; created with `branch: None`, no worktree |
| 3 | N, name, Enter, Esc in the browser, border branch pick X, prompt, Enter | browser opens (repository unconfirmed); Enter on the same repository → X kept → prompt box, no branch list → prompt Enter creates on X (req 7) |
| 4 | N, name, Enter, repository Enter, Esc in the branch list, prompt, Enter | branch list opens (wizard); pick → prompt box; Enter → created |
| 5 | Everything set, prompt in the box, border repository pick B | list closes, `branch` reset, focus prompt box, box text intact; Enter → B's branch list → pick → prompt box → Enter → created (req 5, 6) |
| 6 | Everything set, prompt Enter, then click anything before the session arrives | frozen, row still `new`; the session replaces the row on arrival and is selected |
| 7 | N, name, Enter, repository Enter, branch X, type half a prompt, border model pick | model set, focus prompt box, half prompt intact (req 3) |
| 8 | Default cwd is not Git; name Enter, navigate a plain folder | repository stays unconfirmed; browser only navigates plain folders, and requires a Git repository pick before branch and prompt |
| 9 | N, border repository pick before naming | focus back in the name editor with typed text kept; name Enter → branch list; repository step is already confirmed (req 2) |
| 10 | Named, double-click title, type, Esc | old name kept, draft intact, focus prompt box (req 8) |
| 11 | Everything set, double-click title, new name, Enter | renamed, nothing created, row title updates |
| 12 | Border branch pick X, border repository pick B, prompt, Enter | branch list for B (X invalidated) |
| 13 | Prompt box, nothing open, Esc | delete confirmation; Esc keeps the draft, confirm removes it (req 9) |
| 14 | Branch list (wizard) open, border branch click | reopens as Border; pick → prompt box; Enter → created |
| 15 | Named draft, select another row, come back | row still `new`; box text and images, fields restored; focus prompt box |
| 16 | Named draft selected, another task needs input | attention jumps; the draft stays intact. Unnamed draft selected: no jump (as today) |
| 17 | Named draft with text and a pasted image, Ctrl+C, restart | draft listed as `new`; the box shows the text and the image; Enter continues |
| 18 | Restart after the repository directory was deleted | draft listed with error; Enter with text → directory browser |
| 19 | Restart after branch X was deleted | `branch` `Unchosen`, error notes it; Enter → branch list |
| 20 | Create, worktree creation fails | draft unfrozen with the error, prompt still in the box; change branch, Enter → retry |
| 21 | Create, quit before the session arrives, restart with the session present | draft removed at load |
| 22 | Create, quit, restart with no such session | draft kept with "Creation was interrupted"; Enter retries |
| 23 | Unnamed draft, select another row | dropped |
| 24 | Named draft, Backspace | same confirmation as deleting a session; confirm → gone from list and `agents.json` |
| 25 | Two named drafts | both listed under `new`, newest first; each restores its own box |
| 26 | Non-Git draft, border branch click | no-op |

## Tests

Each fails on the current code unless marked as a guard.

1. Border branch pick X, then wizard repository pick of the same repository from a subdirectory: `branch` still `Existing(X)`; prompt Enter creates with no branch list.
2. Border pick of the checked-out branch, repository confirmed, prompt Enter: `CreateTask` with `branch: None`.
3. Name Enter opens the directory browser (origin Wizard) with the current repository selected; Enter there confirms and the result opens the branch list.
4. Wizard branch pick focuses the prompt box and does not create.
5. Prompt Enter with repository confirmed and branch `Unchosen` opens the branch list; no `CreateTask`.
6. Prompt typed, border repository pick: name editor closed, box text intact, focus prompt box.
7. Branch X chosen, repository result for B: `branch` `Unchosen`, border shows B's checked-out branch.
8. Everything set, border repository pick: no `CreateTask`; next Enter opens B's branch list.
9. Wizard browser Esc and wizard branch list Esc: nothing created, box content kept, focus prompt box; second Esc opens the delete confirmation.
10. Double-click title on a named draft prefills the editor; Esc keeps the name.
11. Name edit Enter, model pick, border branch pick with all fields set: no `CreateTask`.
12. Non-Git default cwd keeps the repository unconfirmed; prompt Enter opens the wizard browser even if the stored confirmation is true. A non-Git directory result is rejected without changing the fields; load of a directory that lost Git resets confirmation and branch, with an error.
13. Named draft appears in `rows` with `SessionStatus::New`, sorted after `NeedsInput`, in `Active` only.
14. Select another row and back: fields, box text, images and focus restored.
15. `reconcile_attention` leaves a selected named draft for a `NeedsInput` task; an unnamed one still blocks the jump.
16. `Preferences` round-trips `state.drafts`; unnamed drafts are not written; a malformed entry is dropped while pinned and usage survive.
17. A pasted image in a named draft is preserved; after reload the attachment is rebuilt from the stored path; a missing file is dropped.
18. Load with missing cwd: `repository_confirmed` false, error set. Missing branch: `Unchosen`. `job` with a matching session: removed; without: "Creation was interrupted".
19. Worktree/create error: draft stays, `working` false, `error` set, box content kept.
20. Session carrying the job arrives: draft removed from rows and state, session selected.
21. Backspace on a draft row opens `ConfirmDialog`; confirm removes and persists without emitting `Action::Delete`.
22. Repository result for a frozen draft is ignored; a second prompt Enter while frozen is ignored.
23. Right-click in the name editor on a named draft discards only the edit.
24. Existing `Step::*` assertions rewritten against fields.

Guards (pass today, kept): non-Git border branch click is a no-op; repository pick before naming keeps the typed name.

## Holes fixed vs v2

- Order is name → repository → branch → prompt; the prompt box content is the prompt, so no saved-prompt/box split and no "continue" key: Enter with text is the only creator.
- Branch is `Unchosen | Existing | New`; the checked-out branch is an ordinary `Existing` pick.
- Repository is a confirmed step with its own flag; a border pick counts; a same-root pick keeps the chosen branch.
- Named drafts are rows with status `new`, delete-only, persisted with box text and preserved image files; unnamed drafts stay transient.
- Whole-draft cancel is the delete confirmation, consistent with deleting a session.
- Creation failure keeps the draft with its error; success removes it only when the session arrives; interrupted creation is repaired at load; no visual creating state.
