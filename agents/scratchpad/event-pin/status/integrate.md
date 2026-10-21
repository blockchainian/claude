# Integration status

Final gate (HEAD f0f4f8dc): `cargo nextest run --no-fail-fast` 1953 run, 1953 passed, 0 failed, 150 skipped (three runs). `cargo build --release` ok. `cargo clippy --all-targets`: no new warnings; the set equals fd00a628's (checked against a clean export of fd00a628), minus one unused-import warning that is gone.

## Done
- Full gate was already green after W1-W5 (1950/1950).
- Real bug fixed in `src/app.rs` `handle_event`: explicit input that moved the selection between pinned rows (only `pinned_cursor` changes, `cursor` stays `None`) did not close the terminal view. It now compares `(cursor, pinned_cursor)`. Test `moving_between_pinned_events_closes_the_terminal` in `src/app_terminal.rs` failed before the fix.
- Test gaps from design.md closed: offset past the transcript end forgets the pin (`stored_pins_with_an_offset_past_the_transcript_are_forgotten`); a forgotten inactive task is not written again by later refreshes (added to `inactive_sessions_lose_their_stored_state`); a pinned running tool keeps its snapshot when the result arrives and re-pinning takes the result (`a_pinned_running_tool_keeps_its_snapshot_when_the_result_arrives`, real `claude-tool-calls.jsonl` lines).
- All other design.md test items were already covered by W1-W5 tests (checked by reading them).

## Not done / notes
- Flaky, pre-existing code: `ui::render::title_input_tests::new_task_name_keeps_its_task_list_color_while_shown_and_edited` failed once in one full run (passes alone and in the next three full runs). It comes from fd00a628's commit 3e4ce42f; not touched.
- W4: pinned rows are not part of drag text selection; PageUp/PageDown step uses the full panel height. Not changed.
- `cargo fmt --check` is already dirty on fd00a628 in files this feature touched; not reformatted.
