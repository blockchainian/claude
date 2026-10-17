# Claude AskUserQuestion key map

Claude Code 2.1.302; Haiku 5.5, low effort; disposable `claude --bg` jobs `8fe544e0` and `13879052`; isolated temporary git directory; built-in tools restricted to AskUserQuestion, external MCP disabled. Two real `claude attach` clients in 200-column × 50-row tmux PTYs. Screens are verbatim terminal-emulator captures in this directory. No personal prompts, credentials, account IDs, or user paths are in the fixtures. The only user keybinding is unrelated `ctrl+q` in Global context.

## Single select

Initial focus: `❯ 1. Red`. Rows in order: Red, Green, Blue, `Type something.`, then a separate `Chat about this` row.

| Key | Observed result |
| --- | --- |
| Down | Move to the next row. Green after one Down from Red. From Type something, move to Chat about this; further Down stays there. |
| Up | Previous row. Up from initial Red wraps to Type something (not Chat about this). Up from Chat about this goes to Type something. |
| 1 / 2 / 3 | On the option list, immediately answer with that label; no extra Enter. `2` answered Green even with Blue highlighted. While Chat about this was focused, `2` had no effect. |
| Space | No effect on a normal single-select option; does not submit. |
| Enter | Confirm highlighted option immediately. Down to Green then Enter answered Green in the two-question call. |
| Tab | No effect for one single-select question. |
| Esc | Reject the tool call and interrupt the turn. Screen: `Interrupted · What should Claude do instead?`; returns to prompt. |

Evidence: 01–03, 06, 08, 11, 34–39 and transcript.jsonl. Both clients share the same highlighted row: second client attached with Green highlighted, second client Up moved both screens to Red, first client Enter answered Red (40–42).

## Multi select

One multiSelect question already has tabs `Multi`, `Submit`. Initial focus Red; all checkboxes empty. An unnumbered `Submit` row appears below Type something and above Chat about this.

| Key | Observed result |
| --- | --- |
| 1 / 2 / 3 | Toggle that numbered checkbox without moving focus or advancing. |
| Up / Down | Move focus without changing checkboxes. |
| Space | Toggle focused checkbox; a second Space untoggles it. |
| Enter on an option | Toggle focused checkbox, just like Space. Does NOT confirm the question. |
| Down to the unnumbered Submit row, Enter | Commit the current question and advance to the review page. |
| Tab | Advance to review and commit selected options: a fresh probe with Red checked showed Red on review. With no selected options, review warned `You have not answered all questions`. Another Tab at the last page had no effect. |
| Enter on review / digit 1 | Submit all committed answers. |

Confirmed two selections by: digit 1, digit 3, Enter on Red (untoggle), digit 1 (retoggle), Down ×4 to Submit, Enter, Enter on review. Review and transcript answered `Blue, Red`: selection insertion order, not option order. The first Tab experiment had cleared all checkboxes before navigating and submitted an empty answer; Claude then repeated the multi question. No answer was fabricated in fixtures.

Evidence: 13–17, 21–24, 43–44; transcript includes both empty and successful multi results. tab-transcript.jsonl records Red selected with digit 1 and submitted via Tab then Enter.

## Two questions in one call

Initial focus is the first question, Red. Header tabs: First, Second, Submit. Tab advances to the second question; Shift-Tab returns to the first. Digit 2 on First commits Green and advances to Second, reset to Red. Down then Enter on Second commits Green and advances to review. Review shows both questions. Enter there submits ONE tool_result containing both answers. No partial tool_result is emitted after answering only First.

Evidence: 25–29 and transcript.jsonl.

## Other / free text

Digit 4 focuses the inline Type something row; it does not answer or open a separate modal. Bracketed paste replaces that row with `custom probe note`. Enter submits the text for a single-select question. The answer map stores the raw text, without `user_note:` or the literal label Type something. Text input becomes active while that row is focused; do not send option shortcut digits there and assume they still choose labels.

Evidence: 30–32 and transcript.jsonl.

## Tool results

Exact tool_result content is in transcript.jsonl, paired by tool_use_id. `toolUseResult` contains full original questions, `answers` keyed by the exact question text, and empty `annotations` for successful answers.

| Call | `toolUseResult.answers` or rejection |
| --- | --- |
| Single | `{ "Probe single choice?": "Green" }` |
| Multi with no selections | `{ "Probe multiple choices?": "" }`; content `The user did not answer the questions.` |
| Multi confirmed via Submit row | `{ "Probe multiple choices?": "Blue, Red" }` |
| Two questions | `{ "Probe first page?": "Green", "Probe second page?": "Green" }` |
| Other | `{ "Probe custom text?": "custom probe note" }` |
| Esc | `is_error: true`; `toolUseResult: "User rejected tool use"` |
| Single via second attach | `{ "Probe space selection?": "Red" }` |

## Pending job state and registry

While pending, registry has `kind: "bg"`, `status: "waiting"`, `waitingFor: "input needed"`. See pending-registry.json for the exact recorded value.

Job state remained `state: "working"`, `detail: "starting…"`, `tempo: "blocked"`. `needs` was `answer: Probe escape? (Red · Green · Blue)` for Escape and `answer: Probe single choice? (Red · Green · Blue)` for Single. `block.questions` also exists, containing question text and options with label/description. That block omits header, multiSelect, and preview, so it cannot reproduce a full Ask.

Crucial persistence observation: while Escape was visible and registry waiting, actual assistant tool_use records contained only Single, Multi twice, First/Second, Custom. Escape was absent. The pending Multi call was likewise absent until it had a tool_result. In this version/run the current call was persisted only after answering/rejecting it. Thus transcript-only Ask parsing is correct for a persisted pending call, but does not guarantee a usable live pending Ask. Do not invent missing header/multiSelect/preview from the job block.

## Differences from the report

- Multi-select Enter toggles an option; advance via the Submit row or Tab, then confirm on review. The expected “Space toggles, Enter confirms” shortcut is false here.
- Single-select Space does not select/commit.
- Other is inline, not a separate text dialog.
- MultiSelect answers are a comma-joined string in selection insertion order, not a vector or sorted labels; custom text is raw, unlike Codex's `user_note:` convention.
- Pending state has populated needs plus a structured but incomplete block. Registry waiting is confirmed.
- A second attach client works; both see and control shared focus.
- A pending tool_use is not necessarily persisted while waiting; live transcript-only detection is incomplete on this run.

Permission prompts and ExitPlanMode were not exercised: this phase restricted tools to AskUserQuestion. No answer transport or UI is implemented.

Cleanup: both probe jobs stopped with `claude stop` and removed with `claude rm`; isolated tmux servers terminated. No persistent Claude job remains.
