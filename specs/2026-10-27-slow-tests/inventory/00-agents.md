| Run-time (s) | Module | Test | Fixable | How |
|---:|---|---|---|---|
| 2.00 (timed out) | ui::tests::focus_rules_tests | rule_8_draft_branch_editor_updates_same_panel_focus | | |
| 1.00 | state::tests | concurrent_processes_preserve_independent_fields | | |

## tmux probes

From one parallel run of `cargo nextest run --run-ignored only` without the slow-timeout. Failing probes are not listed.

| Run-time (s) | Module | Test | Fixable | How |
|---:|---|---|---|---|
| 25.79 | agents::providers::codex::app_server::tests | delete_real_codex_sessions |  | real codex sessions |
| 19.74 | terminal_layout::terminal_scripts | terminal_shell_file_read_colors | fixable | split per file kind |
| 19.66 | terminal_layout::terminal_drafts | terminal_draft_defaults_and_inherited_choices | partly fixable | split defaults, inherited<br>wait_for, not sleep |
| 15.83 | agents::providers::codex::terminal_live_tests | idle_terminal_archives_real_codex_thread_without_hiding_it |  | real codex thread |
| 15.61 | terminal_layout | terminal_layout_tracks_width | fixable | wait_for, not sleep<br>split per width |
| 15.00 | agents::providers::codex::app_server::tests | reject_real_codex_question_interrupts_turn |  | real codex turn |
| 14.67 | terminal_layout::terminal_output_follow | terminal_output_files_follow_appends_and_preserve_reading_position | partly fixable | wait_for, not sleep<br>split follow, position |
| 11.36 | agents::providers::codex::app_server::tests | rename_real_codex_thread |  | real codex thread |
| 11.32 | agents::providers::claude::settings_tests | settings_real_claude_session |  | real claude session |
| 8.78 | terminal_composer | readline_words_match_in_composer | fixable | split per word class |
| 8.62 | agents::providers::codex::app_server::tests | approval_status_marks_a_live_codex_session |  | real codex app-server |
| 6.41 | agents::app::session_terminal::tests | real_woken_terminal_closes_back_to_inactive |  | real claude job startup |
| 5.98 | terminal_layout::terminal_drafts | terminal_draft_wizard_focus_order_and_clicks | partly fixable | split focus order, clicks<br>wait_for, not sleep |
| 5.49 | terminal_layout::terminal_event_scroll | terminal_send_preserves_reading_position | partly fixable | split scroll, send<br>wait_for, not sleep |
| 5.42 | terminal_layout | terminal_running_tool_animates_without_input | partly fixable | wait_for, not 1s sleep<br>3 real spinner frames |
| 4.75 | agents::providers::claude::attach::tests | real_model_command_without_confirmation |  | real `claude --bg` run |
| 4.47 | agents::ui::event_list::event_markdown | markdown_frame_timing | fixable | 100 frames, not 1000 |
| 3.90 | terminal_layout | terminal_task_details_section_positions | fixable | wait_for, not sleep<br>split per fixture |
| 3.87 | terminal_layout | terminal_branch_name_edits_in_task_details_and_preserves_input | partly fixable | split edit, preserved input<br>git init stays |
| 3.22 | terminal_layout::terminal_model_families | terminal_model_families_keep_headers_out_of_selection | fixable | one fixture, not 3<br>split headers, selection |
| 3.13 | terminal_layout::terminal_agent_messages | terminal_codex_collaboration_navigation | partly fixable | one fixture, not 4<br>split per step |
| 2.87 | terminal_layout | terminal_update_click_selects_source_row | fixable | wait_for, not 1s sleeps |
| 2.73 | terminal_layout | terminal_configuration_version_notices |  |  |
| 2.72 | terminal_composer | xterm_tmux_keeps_fallbacks_and_dashboard_keys | partly fixable | split fallbacks, dashboard keys |
| 2.66 | terminal_composer | enhanced_keys_newlines_and_vertical_caret_in_tmux | partly fixable | split per edit kind |
| 2.57 | terminal_layout::terminal_codex_exit | terminal_codex_shell_titles |  |  |
| 2.55 | terminal_layout::paths | terminal_cwd_relative_paths |  |  |
| 2.54 | terminal_layout::terminal_codex_exit | terminal_failed_plain_output_uses_default_foreground |  |  |
| 2.45 | terminal_layout | terminal_inactive_elapsed_stays_at_last_record |  | 2s real wall-clock tick |
| 2.29 | terminal_layout::terminal_await_output | terminal_background_await_shows_message_without_task_id | fixable | split per start |
| 2.29 | terminal_layout | terminal_context_usage_on_task_details_border | fixable | split per start<br>one fixture each |
| 2.25 | finished_subagent_terminal | terminal_finished_wait_updates_from_metadata | fixable | one fixture, not 4 |
| 2.21 | spawn_agent_terminal | terminal_spawn_agent_claude_receipt_and_model | fixable | one fixture, not 3<br>split receipt, model |
| 2.18 | terminal_layout | terminal_background_event_has_no_marker_and_keeps_its_time | fixable | one fixture, not 2<br>split marker, time |
| 2.13 | terminal_layout::terminal_codex_exit | terminal_codex_completed_subagent_calls | fixable | wait_for, not sleep |
| 2.11 | task_list_keys | task_list_unknown_keys_and_help | fixable | split unknown keys, help |
| 2.06 | terminal_layout::terminal_codex_exit | terminal_codex_exit_sections | fixable | smaller fixture |
| 2.02 | terminal_layout::terminal_chrome_tabs_close | terminal_claude_chrome_tabs_close | fixable | smaller fixture |
| 1.66 | terminal_layout::terminal_drafts | terminal_repository_browser_applies_single_clicks | partly fixable | wait_for, not sleep |
| 1.59 | terminal_layout::terminal_output_borders | terminal_output_border_double_clicks | fixable | split per border |
| 1.39 | terminal_layout::terminal_task_details_borders | terminal_task_details_border_double_clicks | fixable | split per border |
| 1.36 | terminal_layout::terminal_drafts | terminal_draft_delete_focus_allows_repeated_new_task | fixable | split second new-task round |
| 1.31 | worktree_dialog_terminal | worktree_confirmation_options_show_directory_and_ellipsis | fixable | split directory, ellipsis |
| 1.24 | terminal_layout::terminal_heredoc_format | terminal_shell_heredoc_formatting | fixable | one fixture, not 2 |
| 1.08 | terminal_layout | terminal_task_details_borders_keep_status_and_elapsed_separate | fixable | wait_for, not sleep |
| 1.08 | terminal_layout | terminal_task_cursor_keeps_cell_colors | fixable | wait_for, not sleep |
| 1.07 | terminal_layout::terminal_event_detail_width | terminal_preformatted_detail_stays_in_content_column | fixable | smaller fixture |
| 1.07 | terminal_layout | terminal_manager_detach_title | fixable | one fixture, not 2 |
| 1.06 | terminal_layout::terminal_path_display | terminal_task_details_directory_precedes_title_and_branch | partly fixable | split directory, branch<br>wait_for, not sleep |
| 1.04 | web_search_terminal | web_search_details_and_browser_link | fixable | one fixture, not 3<br>split details, link |
