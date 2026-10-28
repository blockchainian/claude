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
| 25.79 | agents::providers::codex::app_server::tests | delete_real_codex_sessions |  | real codex sessions |
| 19.74 | terminal_layout::terminal_scripts | terminal_shell_file_read_colors | fixable | 32 probe starts (0.6s each)<br>split per provider, case |
| 19.66 | terminal_layout::terminal_drafts | terminal_draft_defaults_and_inherited_choices | fixable | split settings, width, named<br>wait_for click, not 200ms sleep |
| 15.83 | agents::providers::codex::terminal_live_tests | idle_terminal_archives_real_codex_thread_without_hiding_it |  | real codex thread |
| 15.61 | terminal_layout | terminal_layout_tracks_width | fixable | wait_for resize, not 1s sleep<br>split per width |
| 15.00 | agents::providers::codex::app_server::tests | reject_real_codex_question_interrupts_turn |  | real codex turn |
| 14.67 | terminal_layout::terminal_output_follow | terminal_output_files_follow_appends_and_preserve_reading_position | partly fixable | split per form<br>wait_for, not 1s+500ms sleeps<br>output follow poll stays |
| 11.36 | agents::providers::codex::app_server::tests | rename_real_codex_thread |  | real codex thread |
| 11.32 | agents::providers::claude::settings_tests | settings_real_claude_session |  | real claude session |
| 8.65 | terminal_composer | readline_words_match_in_composer |  |  |
| 8.62 | agents::providers::codex::app_server::tests | approval_status_marks_a_live_codex_session |  | real codex app-server |
| 6.41 | agents::app::session_terminal::tests | real_woken_terminal_closes_back_to_inactive |  | real claude job startup |
| 5.98 | terminal_layout::terminal_drafts | terminal_draft_wizard_focus_order_and_clicks | fixable | split width, order<br>wait_for click, not 200ms sleep |
| 5.49 | terminal_layout::terminal_event_scroll | terminal_send_preserves_reading_position | partly fixable | split scroll, send<br>wait_for, not sleep |
| 5.42 | terminal_layout | terminal_running_tool_animates_without_input | partly fixable | wait_for first frame, not 1s sleep<br>10x130ms real spinner sampling |
| 4.75 | agents::providers::claude::attach::tests | real_model_command_without_confirmation |  | real `claude --bg` run |
| 3.90 | terminal_layout | terminal_task_details_section_positions | fixable | wait_for resize, not 500ms sleep<br>split per has_pr |
| 3.87 | terminal_layout | terminal_branch_name_edits_in_task_details_and_preserves_input | fixable | split edit, discard, cancel<br>wait_for click, not 200ms sleep |
| 3.22 | terminal_layout::terminal_model_families | terminal_model_families_keep_headers_out_of_selection | fixable | split per provider |
| 3.13 | terminal_layout::terminal_agent_messages | terminal_codex_collaboration_navigation | fixable | split per fixture case |
| 2.96 | terminal_layout::terminal_agent_messages | terminal_codex_agent_message_bodies | | |
| 2.87 | terminal_layout | terminal_update_click_selects_source_row | fixable | wait_for, not two 1s sleeps<br>wait_for click, not 200ms sleep |
| 2.87 | terminal_layout::terminal_event_scroll | terminal_event_scroll_reaches_top_before_handoff | | |
| 2.73 | terminal_layout | terminal_configuration_version_notices | fixable | split per upgraded_at_start |
| 2.57 | terminal_layout::terminal_codex_exit | terminal_codex_shell_titles | fixable | split per fixture |
| 2.55 | terminal_layout::paths | terminal_cwd_relative_paths | fixable | split per tool, width |
| 2.54 | terminal_layout::terminal_codex_exit | terminal_failed_plain_output_uses_default_foreground | fixable | split per fixture |
| 2.46 | web_search_terminal | web_search_details_and_browser_link |  |  |
| 2.45 | terminal_layout | terminal_inactive_elapsed_stays_at_last_record |  | real 2s wall clock, elapsed stays |
| 2.37 | terminal_composer | xterm_tmux_keeps_fallbacks_and_dashboard_keys |  |  |
| 2.36 | terminal_composer | enhanced_keys_newlines_and_vertical_caret_in_tmux |  |  |
| 2.29 | terminal_layout::terminal_await_output | terminal_background_await_shows_message_without_task_id | fixable | split per start |
| 2.29 | terminal_layout | terminal_context_usage_on_task_details_border | fixable | split per start<br>one fixture each |
| 2.18 | terminal_layout | terminal_background_event_has_no_marker_and_keeps_its_time | fixable | one fixture, not 2<br>split marker, time |
| 2.13 | terminal_layout::terminal_codex_exit | terminal_codex_completed_subagent_calls | fixable | wait_for, not sleep |
| 2.06 | terminal_layout::terminal_codex_exit | terminal_codex_exit_sections | fixable | smaller fixture |
| 2.02 | terminal_layout::terminal_chrome_tabs_close | terminal_claude_chrome_tabs_close | fixable | smaller fixture |
| 1.87 | task_list_keys | task_list_unknown_z_and_help |  |  |
| 1.86 | worktree_dialog_terminal | worktree_trunk_delete_options_at_120_columns |  |  |
| 1.71 | worktree_dialog_terminal | worktree_trunk_delete_options_at_80_columns |  |  |
| 1.66 | terminal_layout::terminal_drafts | terminal_repository_browser_applies_single_clicks | fixable | wait_for click, not 200ms sleep |
| 1.59 | terminal_layout::terminal_output_borders | terminal_output_border_double_clicks | fixable | wait_for click, not 200ms sleep |
| 1.39 | terminal_layout::terminal_task_details_borders | terminal_task_details_border_double_clicks | fixable | wait_for click, not 200ms sleep |
| 1.37 | worktree_dialog_terminal | worktree_archive_option_directory_at_80_columns |  |  |
| 1.36 | terminal_layout::terminal_drafts | terminal_draft_delete_focus_allows_repeated_new_task | fixable | split per width |
| 1.34 | worktree_dialog_terminal | worktree_delete_option_ellipsis_at_120_columns |  |  |
| 1.24 | terminal_layout::terminal_heredoc_format | terminal_shell_heredoc_formatting | fixable | one fixture, not 2 |
| 1.08 | terminal_layout | terminal_task_details_borders_keep_status_and_elapsed_separate | fixable | wait_for, not 300ms sleep |
| 1.08 | terminal_layout | terminal_task_cursor_keeps_cell_colors | fixable | 1.07s alone<br>wait_for, not 300ms sleep |
| 1.07 | terminal_layout::terminal_event_detail_width | terminal_preformatted_detail_stays_in_content_column | fixable | smaller fixture |
| 1.07 | terminal_layout | terminal_manager_detach_title | fixable | one fixture, not 2 |
| 1.06 | terminal_layout::terminal_path_display | terminal_task_details_directory_precedes_title_and_branch | partly fixable | split directory, branch<br>wait_for, not sleep |

## Failing probes

Probes that failed in a parallel run without the slow-timeout, with the time they failed at.

| Run-time (s) | Module | Test |
|---:|---|---|
| 20.99 | terminal_theme_colors | terminal_theme_colors_follow_bundled_and_custom_themes |
| 7.55 | terminal_layout::terminal_agent_messages | terminal_claude_agent_exchange |
| 7.20 | terminal_layout::terminal_event_scroll | terminal_scrollbar_uses_right_edge_blocks_below_rule |
| 7.10 | terminal_composer | failed_send_shows_its_reason_in_red_in_tmux |
| 6.83 | terminal_layout | terminal_configuration_version_notices |
| 6.53 | terminal_layout::terminal_selection_display | terminal_drag_copies_display_lines_and_joins_markdown_wraps |
| 6.44 | terminal_layout::terminal_task_navigation | terminal_task_navigation_and_opening_keys |
| 6.43 | terminal_layout::terminal_drafts | terminal_draft_escape_follows_task_details_and_prompt_focus |
| 6.38 | terminal_layout::terminal_tool_list | terminal_claude_tool_list_names_groups_and_paths |
| 6.07 | terminal_layout::terminal_task_follow | terminal_task_follow_only_returns_to_active |
| 5.87 | terminal_layout::terminal_task_navigation | terminal_task_border_double_clicks |
| 5.82 | terminal_layout::terminal_task_follow | terminal_pinned_separator_is_centered_and_not_selectable |
| 5.78 | terminal_layout | terminal_duration_sort_jump_position |
| 5.75 | terminal_layout | terminal_event_metadata_aligns_new_with_clock |
| 5.72 | terminal_layout | terminal_filtered_event_labels_and_typed_prompts |
| 5.69 | terminal_layout::terminal_transcript | terminal_transcript_modes |
| 5.68 | terminal_layout | terminal_prompt_marker_caret_and_mouse_alignment |
| 5.68 | terminal_layout::terminal_window_navigation | terminal_vim_window_navigation_uses_rendered_panels |
| 5.65 | terminal_layout::terminal_event_scroll | terminal_event_scroll_window_and_boundaries |
| 5.61 | terminal_layout::terminal_drafts | terminal_title_blur_saves_and_unchanged_enter_closes |
| 5.61 | terminal_layout::terminal_window_navigation | terminal_active_panel_paging_stays_in_focused_panel |
| 5.61 | terminal_layout::terminal_event_search | terminal_event_search_finds_trailing_tool_after_unicode |
| 5.58 | terminal_layout | terminal_event_border_double_clicks |
| 5.58 | terminal_layout::terminal_drafts | terminal_new_task_status_is_capitalized_in_list_and_task_details |
| 5.56 | terminal_layout::terminal_questions | terminal_question_footer_styles_and_clicks |
| 5.52 | terminal_layout::terminal_mermaid | terminal_mermaid_diagrams_and_fallback |
| 5.51 | terminal_layout::terminal_notification | terminal_notification_transcripts_show_only_the_report |
| 5.50 | terminal_layout::terminal_headings | terminal_parallel_tools_have_only_section_titles |
| 5.50 | terminal_layout::terminal_notification | terminal_background_notifications_shorten_titles_and_color_exit_numbers |
| 5.50 | terminal_layout::terminal_headings | terminal_event_section_titles_and_scrolling |
| 5.49 | terminal_layout::terminal_prompt_mouse | terminal_prompt_mouse_click_places_caret |
| 5.48 | terminal_layout::terminal_images | terminal_image_path_and_clipboard_are_atomic |
| 5.48 | terminal_layout::terminal_drafts | terminal_task_names_edit_on_task_details_rule |
| 5.48 | terminal_layout::terminal_drafts | terminal_draft_name_blur_persists_without_enter |
| 5.48 | terminal_layout::terminal_questions | terminal_shared_footer_clicks_and_help_keys |
| 5.47 | terminal_layout | terminal_prompt_kill_copies_and_ctrl_y_pastes |
| 5.46 | terminal_layout::terminal_images | terminal_middle_paste_uses_clicked_input |
| 5.46 | terminal_layout::terminal_model_command | terminal_model_command_refreshes_without_an_assistant_reply |
| 5.46 | terminal_layout | terminal_prompt_newline_entry |
| 5.46 | terminal_layout | terminal_task_notification_shows_summary_without_id_and_missing_file |
| 5.46 | terminal_layout::terminal_task_follow | terminal_status_tabs_filter_children_and_preserve_ancestor_status |
| 5.45 | terminal_layout::terminal_prompt_cache | terminal_prompt_cache_countdown_on_task_details_border |
| 4.85 | terminal_layout::terminal_link_overlays | terminal_confirmation_border_covers_wide_text |
| 3.28 | terminal_layout | terminal_detail_reading_column_preserves_original_copy |
| 2.98 | terminal_layout | terminal_json_strings_keep_multiline_alignment_and_colors |
| 2.70 | terminal_layout::terminal_drafts | terminal_submitted_draft_stays_new_while_git_continues_and_failure_keeps_prompt |
| 2.68 | terminal_layout::terminal_link_overlays | terminal_link_overlays_cover_underlines_and_blank_clicks |
| 2.60 | terminal_layout | terminal_leaving_list_search_preserves_event_detail |
| 2.50 | terminal_layout::terminal_drafts | terminal_picker_bottom_layout |
| 2.46 | terminal_layout | terminal_last_activity_heading_stays_centered |
| 2.45 | terminal_layout::terminal_questions | terminal_ask_questions_render_real_rejection_and_answer |
| 2.32 | terminal_layout::terminal_headings | terminal_group_headings_center_at_wide_and_narrow_widths |
| 2.22 | terminal_layout::terminal_event_scroll | terminal_event_scroll_footer_shows_scrolled_off_title |
| 2.22 | terminal_layout | terminal_event_selection_tracks_wheel_click_and_tail |
| 2.06 | terminal_layout::terminal_golden_layout | terminal_large_golden_layout |
| 2.05 | terminal_layout | terminal_compaction_summary_keeps_row_and_labels_event_detail |
| 2.00 | terminal_layout::terminal_event_scroll | terminal_event_scroll_live_appends_keep_short_entries_moving |
| 1.02 | terminal_layout::terminal_task_follow | terminal_inactive_child_pin_lasts_until_root_finishes |
| 0.72 | terminal_layout | terminal_session_search_overlays_chart_bottom |
| 0.66 | terminal_layout::terminal_weekly_usage | terminal_weekly_usage_cache_toggle_and_restart |
| 0.57 | terminal_layout | terminal_notification_json_preserves_lines_wide_and_narrow |
| 0.56 | terminal_layout::terminal_scripts | terminal_bash_interpreter_script_colors |
| 0.56 | terminal_layout::terminal_summary | terminal_summary_styles_match_across_all_three_surfaces |
| 0.52 | terminal_layout::terminal_scripts | terminal_shell_line_format_filters_colors |
| 0.52 | terminal_layout | terminal_start_tool_name_header |
| 0.52 | terminal_layout::terminal_scripts | terminal_shell_reads_before_other_commands_colors |
| 0.51 | terminal_layout::terminal_scripts | terminal_shell_multi_file_read_colors |
| 0.51 | terminal_layout::terminal_scripts | terminal_shell_output_boundaries_colors |
| 0.51 | terminal_layout | terminal_queued_task_details_wrap_and_separate_messages |
| 0.51 | terminal_layout::terminal_structured | terminal_structured_json_xml_and_literal_text |
