## Criteria

Every test should finish in under 1s.

Never:

1. Change product code. If the cause of slowness is in product code, name that code instead of changing it.
2. Cut coverage. Every product behavior a test checks must stay checked.
3. Fake the flow. Don't age timestamps, use fake clocks, skip frames the real app draws, or move the test away from the real flow.

Allowed fixes:

1. Split unrelated behaviors. A test that checks several unrelated product behaviors becomes one test per behavior, and every original assertion survives in exactly one of them. Behaviors of different panels are unrelated, and so are different triggers of the same effect, such as mouse scrolling and keyboard scrolling.
2. Cut data and fixtures. Shrink data or fixtures the behavior doesn't need to the minimum that still tests it. Where the test itself replicates data or repeats operations, cut the copies and the repetitions.
3. Break up combinatorial tests. Replace a test that runs every combination of several code paths with one unit test per path.
4. Remove unneeded waits. Shorten fixed sleeps, polling intervals and waits longer than the event they wait for, or wait for the event itself.

Verdicts:

| Fixable | Meaning |
|---|---|
| fixable | The allowed fixes bring the test, and every test split from it, under 1s. |
| partly fixable | The allowed fixes make it faster, but at least one test stays over 1s. |
| blank | None of the allowed fixes helps. |

## Slow probes

Passing probes over 1s, timed without the slow-timeout: untouched probes from one parallel run, fixed probes from individual runs.

| Run-time (s) | Module | Test | Fixable | How |
|---:|---|---|---|---|
| 4.30 | terminal_layout::terminal_structured | terminal_structured_json_xml_and_literal_text |  |  |
| 3.87 | terminal_layout | terminal_branch_name_edits_in_task_details_and_preserves_input | partly fixable | wait for the branch row, not 200ms sleep<br>poll 10ms |
| 3.84 | terminal_layout | terminal_layout_tracks_width | partly fixable | wait for search focus, not 200ms sleep<br>drop repeated last pass |
| 3.83 | terminal_layout::terminal_weekly_usage | terminal_weekly_usage_cache_toggle_and_restart |  |  |
| 3.63 | terminal_layout | terminal_running_tool_animates_without_input | partly fixable | stop after 3 frames<br>sleep 1 and kill, not wait on sleep 2 |
| 3.29 | terminal_layout | terminal_context_usage_on_task_details_border | partly fixable | split per fixture<br>smaller codex fixture |
| 3.22 | terminal_layout::terminal_model_families | terminal_model_families_keep_headers_out_of_selection | partly fixable | split per provider<br>wait for the picker, not 200ms sleeps |
| 3.10 | terminal_layout | terminal_task_details_section_positions | partly fixable | break the PR state × width grid |
| 2.97 | terminal_layout::terminal_summary | terminal_summary_styles_match_across_all_three_surfaces |  |  |
| 2.87 | terminal_layout::terminal_event_scroll | terminal_event_scroll_reaches_top_before_handoff | partly fixable | wait for the wheel, not 4 × 200ms |
| 2.57 | terminal_layout::terminal_codex_exit | terminal_codex_shell_titles | partly fixable | both threads in one start |
| 2.54 | terminal_layout::terminal_codex_exit | terminal_failed_plain_output_uses_default_foreground | partly fixable | both threads in one start |
| 2.45 | terminal_layout | terminal_inactive_elapsed_stays_at_last_record | partly fixable | 1.1s sleep, not 2s |
| 2.37 | terminal_composer | xterm_tmux_keeps_fallbacks_and_dashboard_keys | partly fixable | poll 10ms, not 50ms |
| 2.36 | terminal_composer | enhanced_keys_newlines_and_vertical_caret_in_tmux | partly fixable | poll 10ms, not 50ms |
| 2.27 | terminal_layout::paths | terminal_cwd_relative_paths | partly fixable | Read and Edit in one transcript |
| 2.04 | terminal_layout::terminal_output_borders | terminal_output_border_double_clicks | partly fixable | split per width<br>drop the no-op resize |
| 2.00 | terminal_layout | terminal_update_click_selects_source_row | partly fixable | select the row by keys, keep one click |
| 1.99 | terminal_layout | terminal_task_details_borders_keep_status_and_elapsed_separate | partly fixable | check 200 before resizing |
| 1.87 | task_list_keys | task_list_unknown_z_and_help | partly fixable | merge h and z into one start |
| 1.87 | terminal_layout::terminal_drafts | terminal_new_task_status_is_capitalized_in_list_and_task_details |  |  |
| 1.86 | worktree_dialog_terminal | worktree_trunk_delete_options_at_120_columns | partly fixable | drop the last checkout<br>trim codex-state.sql |
| 1.71 | worktree_dialog_terminal | worktree_trunk_delete_options_at_80_columns | partly fixable | drop the last checkout<br>trim codex-state.sql |
| 1.59 | terminal_composer | failed_send_shows_its_reason_in_red_in_tmux |  |  |
| 1.46 | terminal_layout | terminal_task_cursor_keeps_cell_colors | partly fixable | check 200 before resizing |
| 1.43 | terminal_layout::terminal_drafts | terminal_repository_browser_applies_single_clicks | partly fixable | drop 5 picker 200ms sleeps |
| 1.37 | worktree_dialog_terminal | worktree_archive_option_directory_at_80_columns | partly fixable | trim codex-state.sql |
| 1.34 | worktree_dialog_terminal | worktree_delete_option_ellipsis_at_120_columns | partly fixable | trim codex-state.sql |
| 1.24 | terminal_layout::terminal_heredoc_format | terminal_shell_heredoc_formatting | fixable | split per provider |
| 1.06 | terminal_layout::terminal_path_display | terminal_task_details_directory_precedes_title_and_branch | fixable | wait for the dialog, not 2 × 200ms |
