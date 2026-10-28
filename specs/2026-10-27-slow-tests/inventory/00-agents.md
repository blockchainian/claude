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

From one parallel run of `cargo nextest run --run-ignored only` without the slow-timeout. Failing probes are not listed.

| Run-time (s) | Module | Test | Fixable | How |
|---:|---|---|---|---|
| 20.14 | terminal_layout::terminal_drafts | terminal_draft_defaults_and_inherited_choices | partly fixable | break the 2×2×2 grid, 80 clicks |
| 19.74 | terminal_layout::terminal_scripts | terminal_shell_file_read_colors | partly fixable | one transcript per provider: 2 starts, not 36 |
| 12.25 | terminal_layout::terminal_model_command | terminal_model_command_refreshes_without_an_assistant_reply |  |  |
| 12.23 | terminal_layout::terminal_task_follow | terminal_status_tabs_filter_children_and_preserve_ancestor_status |  |  |
| 12.17 | terminal_layout::terminal_event_scroll | terminal_event_scroll_footer_shows_scrolled_off_title |  |  |
| 11.32 | agents::providers::claude::settings_tests | settings_real_claude_session |  | real claude session |
| 9.42 | terminal_theme_colors | terminal_theme_colors_follow_bundled_and_custom_themes |  |  |
| 8.65 | terminal_composer | readline_words_match_in_composer | partly fixable | one case per key, not 35<br>poll 10ms |
| 7.73 | terminal_layout | terminal_last_activity_heading_stays_centered |  |  |
| 6.49 | terminal_layout | terminal_compaction_summary_keeps_row_and_labels_event_detail |  |  |
| 6.41 | agents::app::session_terminal::tests | real_woken_terminal_closes_back_to_inactive |  | real claude job startup |
| 5.99 | terminal_layout::terminal_output_follow | terminal_output_files_follow_appends_and_preserve_reading_position | partly fixable | wait, not 500ms and 200ms sleeps |
| 5.98 | terminal_layout::terminal_drafts | terminal_draft_wizard_focus_order_and_clicks | partly fixable | resize, not restart per width |
| 5.49 | terminal_layout::terminal_event_scroll | terminal_send_preserves_reading_position | partly fixable | wait for the wheel, not 5 × 200ms |
| 4.75 | agents::providers::claude::attach::tests | real_model_command_without_confirmation |  | real `claude --bg` run |
| 4.30 | terminal_layout::terminal_structured | terminal_structured_json_xml_and_literal_text |  |  |
| 3.87 | terminal_layout | terminal_branch_name_edits_in_task_details_and_preserves_input | partly fixable | wait for the branch row, not 200ms sleep<br>poll 10ms |
| 3.84 | terminal_layout | terminal_layout_tracks_width | partly fixable | wait for search focus, not 200ms sleep<br>drop repeated last pass |
| 3.83 | terminal_layout::terminal_weekly_usage | terminal_weekly_usage_cache_toggle_and_restart |  |  |
| 3.63 | terminal_layout | terminal_running_tool_animates_without_input | partly fixable | stop after 3 frames<br>sleep 1 and kill, not wait on sleep 2 |
| 3.29 | terminal_layout | terminal_context_usage_on_task_details_border | partly fixable | split per fixture<br>smaller codex fixture |
| 3.22 | terminal_layout::terminal_model_families | terminal_model_families_keep_headers_out_of_selection | partly fixable | split per provider<br>wait for the picker, not 200ms sleeps |
| 3.13 | terminal_layout::terminal_agent_messages | terminal_codex_collaboration_navigation |  | 6 clicks held 300ms by the product |
| 3.10 | terminal_layout | terminal_task_details_section_positions | partly fixable | break the PR state × width grid |
| 2.97 | terminal_layout::terminal_summary | terminal_summary_styles_match_across_all_three_surfaces |  |  |
| 2.96 | terminal_layout::terminal_agent_messages | terminal_codex_agent_message_bodies |  | 4 clicks held 300ms by the product |
| 2.87 | terminal_layout::terminal_event_scroll | terminal_event_scroll_reaches_top_before_handoff | partly fixable | wait for the wheel, not 4 × 200ms |
| 2.57 | terminal_layout::terminal_codex_exit | terminal_codex_shell_titles | partly fixable | both threads in one start |
| 2.54 | terminal_layout::terminal_codex_exit | terminal_failed_plain_output_uses_default_foreground | partly fixable | both threads in one start |
| 2.46 | web_search_terminal | web_search_details_and_browser_link |  | one start, real macOS open |
| 2.45 | terminal_layout | terminal_inactive_elapsed_stays_at_last_record | partly fixable | 1.1s sleep, not 2s |
| 2.37 | terminal_composer | xterm_tmux_keeps_fallbacks_and_dashboard_keys | partly fixable | poll 10ms, not 50ms |
| 2.36 | terminal_composer | enhanced_keys_newlines_and_vertical_caret_in_tmux | partly fixable | poll 10ms, not 50ms |
| 2.29 | terminal_layout::terminal_await_output | terminal_background_await_shows_message_without_task_id |  | one start, no sleeps |
| 2.27 | terminal_layout::paths | terminal_cwd_relative_paths | partly fixable | Read and Edit in one transcript |
| 2.27 | terminal_layout::terminal_task_details_borders | terminal_task_details_border_double_clicks |  | one start, four widths |
| 2.13 | terminal_layout::terminal_codex_exit | terminal_codex_completed_subagent_calls |  | one start, one held click |
| 2.06 | terminal_layout::terminal_codex_exit | terminal_codex_exit_sections |  | one start, one-line fixture |
| 2.05 | terminal_layout | terminal_configuration_version_notices | partly fixable | split start and runtime upgrade |
| 2.04 | terminal_layout::terminal_output_borders | terminal_output_border_double_clicks | partly fixable | split per width<br>drop the no-op resize |
| 2.02 | terminal_layout::terminal_chrome_tabs_close | terminal_claude_chrome_tabs_close |  | one start, no sleeps |
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
| 1.19 | terminal_layout | terminal_background_event_has_no_marker_and_keeps_its_time |  | one start, at the floor |
| 1.07 | terminal_layout::terminal_event_detail_width | terminal_preformatted_detail_stays_in_content_column |  | one start, waits only |
| 1.06 | terminal_layout::terminal_drafts | terminal_draft_delete_focus_allows_repeated_new_task |  | one start, at the floor |
| 1.06 | terminal_layout::terminal_path_display | terminal_task_details_directory_precedes_title_and_branch | fixable | wait for the dialog, not 2 × 200ms |
| 1.02 | terminal_layout | terminal_manager_detach_title |  | one start, at the floor |

## Failing probes

Probes that failed in a parallel run without the slow-timeout, with the time they failed at.

| Run-time (s) | Module | Test | Root cause | Fixable | How |
|---:|---|---|---|---|---|
| 7.55 | terminal_layout::terminal_agent_messages | terminal_claude_agent_exchange |  |  |  |
| 7.20 | terminal_layout::terminal_event_scroll | terminal_scrollbar_uses_right_edge_blocks_below_rule |  |  |  |
| 6.83 | terminal_layout | terminal_configuration_version_notices | passes alone: load or ordering |  |  |
| 6.53 | terminal_layout::terminal_selection_display | terminal_drag_copies_display_lines_and_joins_markdown_wraps |  |  |  |
| 6.44 | terminal_layout::terminal_task_navigation | terminal_task_navigation_and_opening_keys |  |  |  |
| 6.43 | terminal_layout::terminal_drafts | terminal_draft_escape_follows_task_details_and_prompt_focus | after the cwd fix: focus wait times out |  |  |
| 6.38 | terminal_layout::terminal_tool_list | terminal_claude_tool_list_names_groups_and_paths |  |  |  |
| 6.07 | terminal_layout::terminal_task_follow | terminal_task_follow_only_returns_to_active |  |  |  |
| 5.87 | terminal_layout::terminal_task_navigation | terminal_task_border_double_clicks |  |  |  |
| 5.82 | terminal_layout::terminal_task_follow | terminal_pinned_separator_is_centered_and_not_selectable | width wait times out before the separator check |  |  |
| 5.78 | terminal_layout | terminal_duration_sort_jump_position | click at 100 columns lands in the prompt, so `t` is typed |  |  |
| 5.75 | terminal_layout | terminal_event_metadata_aligns_new_with_clock |  |  |  |
| 5.72 | terminal_layout | terminal_filtered_event_labels_and_typed_prompts |  |  |  |
| 5.69 | terminal_layout::terminal_transcript | terminal_transcript_modes |  |  |  |
| 5.68 | terminal_layout | terminal_prompt_marker_caret_and_mouse_alignment | after the cwd fix: `Search ❯ Stop` never shows |  |  |
| 5.68 | terminal_layout::terminal_window_navigation | terminal_vim_window_navigation_uses_rendered_panels | after the title fix: a later wait times out |  |  |
| 5.65 | terminal_layout::terminal_event_scroll | terminal_event_scroll_window_and_boundaries |  |  |  |
| 5.61 | terminal_layout::terminal_drafts | terminal_title_blur_saves_and_unchanged_enter_closes | after the cwd fix: title shows `Saved on title h` |  |  |
| 5.61 | terminal_layout::terminal_window_navigation | terminal_active_panel_paging_stays_in_focused_panel |  |  |  |
| 5.61 | terminal_layout::terminal_event_search | terminal_event_search_finds_trailing_tool_after_unicode |  |  |  |
| 5.58 | terminal_layout | terminal_event_border_double_clicks |  |  |  |
| 5.56 | terminal_layout::terminal_questions | terminal_question_footer_styles_and_clicks |  |  |  |
| 5.52 | terminal_layout::terminal_mermaid | terminal_mermaid_diagrams_and_fallback |  |  |  |
| 5.51 | terminal_layout::terminal_notification | terminal_notification_transcripts_show_only_the_report |  |  |  |
| 5.50 | terminal_layout::terminal_headings | terminal_parallel_tools_have_only_section_titles |  |  |  |
| 5.50 | terminal_layout::terminal_notification | terminal_background_notifications_shorten_titles_and_color_exit_numbers |  |  |  |
| 5.50 | terminal_layout::terminal_headings | terminal_event_section_titles_and_scrolling |  |  |  |
| 5.49 | terminal_layout::terminal_prompt_mouse | terminal_prompt_mouse_click_places_caret | after the cwd fix: mouse edit text differs |  |  |
| 5.48 | terminal_layout::terminal_images | terminal_image_path_and_clipboard_are_atomic |  |  |  |
| 5.48 | terminal_layout::terminal_drafts | terminal_task_names_edit_on_task_details_rule | after the cwd fix: screen returns to the session |  |  |
| 5.48 | terminal_layout::terminal_drafts | terminal_draft_name_blur_persists_without_enter | after the cwd fix: draft name missing after blur |  |  |
| 5.48 | terminal_layout::terminal_questions | terminal_shared_footer_clicks_and_help_keys |  |  |  |
| 5.47 | terminal_layout | terminal_prompt_kill_copies_and_ctrl_y_pastes | after the cwd fix: line and clipboard never match together |  |  |
| 5.46 | terminal_layout::terminal_images | terminal_middle_paste_uses_clicked_input |  |  |  |
| 5.46 | terminal_layout | terminal_prompt_newline_entry | after the cwd fix: `first line` stays after clearing |  |  |
| 5.46 | terminal_layout | terminal_task_notification_shows_summary_without_id_and_missing_file |  |  |  |
| 5.45 | terminal_layout::terminal_prompt_cache | terminal_prompt_cache_countdown_on_task_details_border |  |  |  |
| 4.85 | terminal_layout::terminal_link_overlays | terminal_confirmation_border_covers_wide_text |  |  |  |
| 3.28 | terminal_layout | terminal_detail_reading_column_preserves_original_copy |  |  |  |
| 2.98 | terminal_layout | terminal_json_strings_keep_multiline_alignment_and_colors |  |  |  |
| 2.70 | terminal_layout::terminal_drafts | terminal_submitted_draft_stays_new_while_git_continues_and_failure_keeps_prompt | after the cwd fix: prompt input lost while git hangs |  |  |
| 2.68 | terminal_layout::terminal_link_overlays | terminal_link_overlays_cover_underlines_and_blank_clicks |  |  |  |
| 2.60 | terminal_layout | terminal_leaving_list_search_preserves_event_detail |  |  |  |
| 2.50 | terminal_layout::terminal_drafts | terminal_picker_bottom_layout | after the cwd fix: model list bottom row differs |  |  |
| 2.45 | terminal_layout::terminal_questions | terminal_ask_questions_render_real_rejection_and_answer |  |  |  |
| 2.32 | terminal_layout::terminal_headings | terminal_group_headings_center_at_wide_and_narrow_widths |  |  |  |
| 2.22 | terminal_layout | terminal_event_selection_tracks_wheel_click_and_tail | after the footer fix: first detail is User, not decision |  |  |
| 2.06 | terminal_layout::terminal_golden_layout | terminal_large_golden_layout |  |  |  |
| 2.00 | terminal_layout::terminal_event_scroll | terminal_event_scroll_live_appends_keep_short_entries_moving |  |  |  |
| 1.02 | terminal_layout::terminal_task_follow | terminal_inactive_child_pin_lasts_until_root_finishes | after reading `inactive`: child selection wait times out |  |  |
| 0.72 | terminal_layout | terminal_session_search_overlays_chart_bottom |  |  |  |
| 0.57 | terminal_layout | terminal_notification_json_preserves_lines_wide_and_narrow |  |  |  |
| 0.56 | terminal_layout::terminal_scripts | terminal_bash_interpreter_script_colors | after the text fix: command and path share one color |  |  |
| 0.52 | terminal_layout::terminal_scripts | terminal_shell_line_format_filters_colors | blank row between command output sections |  | decide: stale test or product bug |
| 0.52 | terminal_layout | terminal_start_tool_name_header |  |  |  |
| 0.52 | terminal_layout::terminal_scripts | terminal_shell_reads_before_other_commands_colors |  |  |  |
| 0.51 | terminal_layout::terminal_scripts | terminal_shell_multi_file_read_colors | blank row between command output sections |  | decide: stale test or product bug |
| 0.51 | terminal_layout::terminal_scripts | terminal_shell_output_boundaries_colors | blank row between command output sections |  | decide: stale test or product bug |
| 0.51 | terminal_layout | terminal_queued_task_details_wrap_and_separate_messages |  |  |  |
