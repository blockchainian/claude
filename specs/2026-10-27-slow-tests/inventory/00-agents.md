| Run-time (s) | Module | Test | Fixable | How |
|---:|---|---|---|---|
| 2.00 (timed out) | ui::tests::focus_rules_tests | rule_8_draft_branch_editor_updates_same_panel_focus | | |
| 1.00 | state::tests | concurrent_processes_preserve_independent_fields | | |

## tmux probes

Run with `cargo nextest run --run-ignored only`. Probes that hit the 2s timeout were re-run in parallel without it; "failed" means the probe failed after that time.

| Run-time (s) | Module | Test | Fixable | How |
|---:|---|---|---|---|
| 32.94 (failed) | terminal_layout::terminal_drafts | terminal_draft_escape_follows_task_details_and_prompt_focus | partly fixable | fails: unclear<br>split details, prompt<br>wait_for, not sleep |
| 32.74 (failed) | terminal_layout::terminal_agent_messages | terminal_claude_agent_exchange | partly fixable | fails: unclear<br>one fixture, not 4<br>wait_for, not sleep |
| 32.43 (failed) | terminal_layout | terminal_duration_sort_jump_position | partly fixable | fails: unclear<br>wait_for, not sleep |
| 32.39 (failed) | terminal_layout | terminal_event_metadata_aligns_new_with_clock | partly fixable | fails: unclear<br>split marker, clock<br>wait_for, not sleep |
| 32.37 (failed) | terminal_layout::terminal_event_scroll | terminal_event_scroll_window_and_boundaries | partly fixable | fails: unclear<br>split window, boundaries<br>wait_for, not sleep |
| 32.23 (failed) | terminal_layout::terminal_drafts | terminal_title_blur_saves_and_unchanged_enter_closes | fixable | fails: unclear<br>split blur, enter |
| 32.22 (failed) | terminal_layout | terminal_event_border_double_clicks | fixable | fails: real<br>wait_for, not sleep |
| 32.14 (failed) | terminal_layout::terminal_drafts | terminal_task_names_edit_on_task_details_rule | partly fixable | fails: unclear<br>split per edit path<br>drop 600ms sleeps |
| 32.10 (failed) | terminal_layout::terminal_drafts | terminal_new_task_status_is_capitalized_in_list_and_task_details | fixable | fails: unclear<br>near startup floor |
| 32.10 (failed) | terminal_layout::terminal_drafts | terminal_draft_name_blur_persists_without_enter | partly fixable | fails: unclear<br>wait_for, not sleep |
| 32.06 (failed) | terminal_layout | terminal_configuration_version_notices | fixable | fails: unclear<br>near startup floor |
| 31.64 (failed) | terminal_layout::terminal_task_navigation | terminal_task_navigation_and_opening_keys | partly fixable | fails: unclear<br>split navigation, opening |
| 31.64 (failed) | terminal_layout::terminal_selection_display | terminal_drag_copies_display_lines_and_joins_markdown_wraps | partly fixable | fails: unclear<br>split lines, markdown<br>drop 350ms sleep |
| 31.40 (failed) | terminal_layout::terminal_tool_list | terminal_claude_tool_list_names_groups_and_paths | fixable | fails: unclear<br>one fixture, not 3<br>split names, groups, paths |
| 31.11 (failed) | terminal_layout::terminal_task_follow | terminal_task_follow_only_returns_to_active | fixable | fails: unclear<br>split return, no return |
| 30.86 (failed) | terminal_layout::terminal_task_navigation | terminal_task_border_double_clicks | fixable | fails: unclear<br>wait_for, not sleep |
| 30.85 (failed) | terminal_layout::terminal_task_follow | terminal_pinned_separator_is_centered_and_not_selectable | partly fixable | fails: unclear<br>split centered, selectable<br>wait_for, not sleep |
| 30.69 (failed) | terminal_layout::terminal_window_navigation | terminal_vim_window_navigation_uses_rendered_panels | fixable | fails: unclear<br>split per direction |
| 30.67 (failed) | terminal_layout::terminal_transcript | terminal_transcript_modes | partly fixable | fails: unclear<br>split per mode |
| 30.66 (failed) | terminal_layout | terminal_prompt_marker_caret_and_mouse_alignment | partly fixable | fails: real<br>split caret, mouse |
| 30.65 (failed) | terminal_layout | terminal_filtered_event_labels_and_typed_prompts | fixable | fails: unclear<br>split labels, prompts |
| 30.64 (failed) | terminal_layout::terminal_window_navigation | terminal_active_panel_paging_stays_in_focused_panel | partly fixable | fails: unclear<br>split per panel |
| 30.61 (failed) | terminal_layout::terminal_questions | terminal_question_footer_styles_and_clicks | fixable | fails: unclear<br>split styles, clicks |
| 30.51 (failed) | terminal_layout::terminal_prompt_mouse | terminal_prompt_mouse_click_places_caret | partly fixable | fails: unclear<br>split per click target<br>wait_for, not sleep |
| 30.49 (failed) | terminal_layout::terminal_notification | terminal_notification_transcripts_show_only_the_report | fixable | fails: unclear<br>one fixture, not 4 |
| 30.49 (failed) | terminal_layout::terminal_prompt_cache | terminal_prompt_cache_countdown_on_task_details_border | fixable | fails: unclear<br>split per wall time |
| 30.49 (failed) | terminal_layout | terminal_prompt_newline_entry | fixable | fails: real<br>near startup floor |
| 30.48 (failed) | terminal_layout::terminal_headings | terminal_parallel_tools_have_only_section_titles | fixable | fails: unclear<br>near startup floor |
| 30.48 (failed) | terminal_layout::terminal_images | terminal_image_path_and_clipboard_are_atomic | fixable | fails: unclear<br>split path, clipboard |
| 30.48 (failed) | terminal_layout | terminal_task_notification_shows_summary_without_id_and_missing_file | fixable | fails: unclear<br>split summary, missing file |
| 30.47 (failed) | terminal_layout::terminal_headings | terminal_event_section_titles_and_scrolling | partly fixable | fails: unclear<br>split titles, scrolling<br>one fixture |
| 30.47 (failed) | terminal_layout::terminal_images | terminal_middle_paste_uses_clicked_input | partly fixable | fails: unclear<br>split per input<br>wait_for, not sleep |
| 30.47 (failed) | terminal_layout::terminal_model_command | terminal_model_command_refreshes_without_an_assistant_reply | fixable | fails: unclear<br>one fixture, not 2 |
| 30.47 (failed) | terminal_layout | terminal_prompt_kill_copies_and_ctrl_y_pastes | fixable | fails: real<br>near startup floor |
| 30.47 (failed) | terminal_layout::terminal_questions | terminal_shared_footer_clicks_and_help_keys | fixable | fails: unclear<br>split clicks, help keys |
| 30.47 (failed) | terminal_layout::terminal_notification | terminal_background_notifications_shorten_titles_and_color_exit_numbers | fixable | fails: unclear<br>split titles, exit colors |
| 30.46 (failed) | terminal_layout::terminal_mermaid | terminal_mermaid_diagrams_and_fallback | fixable | fails: unclear<br>split diagram, fallback |
| 30.46 (failed) | terminal_layout::terminal_task_follow | terminal_status_tabs_filter_children_and_preserve_ancestor_status | partly fixable | fails: unclear<br>split per tab |
| 30.45 (failed) | terminal_layout | terminal_weekly_usage_on_task_header | fixable | fails: unclear<br>near startup floor |
| 25.88 | agents::providers::codex::app_server::tests | delete_real_codex_sessions |  | real codex sessions |
| 21.23 | terminal_layout::terminal_drafts | terminal_draft_defaults_and_inherited_choices | partly fixable | split defaults, inherited<br>wait_for, not sleep |
| 17.35 | terminal_layout::terminal_scripts | terminal_shell_file_read_colors | fixable | split per file kind |
| 15.48 | terminal_layout | terminal_layout_tracks_width | fixable | wait_for, not sleep<br>split per width |
| 15.34 | agents::providers::codex::app_server::tests | reject_real_codex_question_interrupts_turn |  | real codex turn |
| 14.55 | terminal_layout::terminal_output_follow | terminal_output_files_follow_appends_and_preserve_reading_position | partly fixable | wait_for, not sleep<br>split follow, position |
| 13.42 (failed) | agents::providers::codex::app_server::tests | stop_and_archive_real_codex_thread |  | fails: real<br>real codex thread |
| 12.38 | agents::providers::codex::terminal_live_tests | idle_terminal_archives_real_codex_thread_without_hiding_it |  | real codex thread |
| 12.31 | agents::providers::claude::rename_live_tests | rename_real_socket_session |  | real socket session |
| 11.10 | agents::providers::claude::attach_live_tests | real_terminal_panel_clears_left_input |  | real claude session |
| 10.81 | agents::providers::claude::settings_tests | settings_real_claude_session |  | real claude session |
| 10.74 (failed) | agents::providers::claude::attach_live_tests | real_attach_prompt_paste_busy_steer_interrupt_model |  | fails: real<br>real claude session |
| 9.12 | agents::providers::codex::app_server::tests | rename_real_codex_thread |  | real codex thread |
| 9.08 | terminal_composer | readline_words_match_in_composer | fixable | split per word class |
| 8.44 (failed) | agents::providers::codex::settings_tests | settings_real_codex_thread |  | fails: real<br>real codex thread |
| 8.24 | agents::providers::codex::app_server::tests | approval_status_marks_a_live_codex_session |  | real codex app-server |
| 8.14 (failed) | agents::providers::codex::ask_live_tests | answer_real_codex_question_in_dashboard |  | fails: real<br>real codex question |
| 7.88 (failed) | agents::providers::codex::send_tests | send_real_codex_thread |  | fails: real<br>real codex send |
| 7.50 | terminal_layout::terminal_drafts | terminal_draft_wizard_focus_order_and_clicks | partly fixable | split focus order, clicks<br>wait_for, not sleep |
| 6.91 | terminal_layout::terminal_event_scroll | terminal_send_preserves_reading_position | partly fixable | split scroll, send<br>wait_for, not sleep |
| 6.48 (failed) | agents::providers::claude::attach_live_tests::subagent_live_tests | real_dashboard_prompt_reaches_running_subagent_and_restores_main |  | fails: real<br>real claude subagent |
| 5.80 (failed) | agents::providers::claude::daemon_socket::live_tests | real_background_input_client_and_dashboard |  | fails: real<br>real daemon socket |
| 5.44 | terminal_layout | terminal_running_tool_animates_without_input | partly fixable | wait_for, not 1s sleep<br>3 real spinner frames |
| 4.42 | agents::app::session_terminal::tests | real_woken_terminal_closes_back_to_inactive |  | real claude job startup |
| 4.04 | agents::providers::claude::attach::tests | real_model_command_without_confirmation |  | real `claude --bg` run |
| 3.93 | terminal_layout | terminal_context_usage_on_task_details_border | fixable | split per start<br>one fixture each |
| 3.91 | terminal_layout | terminal_task_details_section_positions | fixable | wait_for, not sleep<br>split per fixture |
| 3.25 (failed) | agents::ui::tests | task_header_weekly_usage_live_accounts |  | fails: real<br>live account fetch |
| 3.18 | terminal_layout::terminal_model_families | terminal_model_families_keep_headers_out_of_selection | fixable | one fixture, not 3<br>split headers, selection |
| 2.93 | agents::ui::event_list::event_markdown | markdown_frame_timing | fixable | 100 frames, not 1000 |
| 2.87 | terminal_composer | enhanced_keys_newlines_and_vertical_caret_in_tmux | partly fixable | split per edit kind |
| 2.87 | terminal_layout | terminal_update_click_selects_source_row | fixable | wait_for, not 1s sleeps |
| 2.45 | terminal_layout | terminal_inactive_elapsed_stays_at_last_record |  | 2s real wall-clock tick |
| 2.00 | spawn_agent_terminal | terminal_spawn_agent_claude_receipt_and_model | fixable | one fixture, not 3<br>split receipt, model |
| 1.95 | terminal_layout | terminal_branch_name_edits_in_task_details_and_preserves_input | partly fixable | split edit, preserved input<br>git init stays |
| 1.95 | finished_subagent_terminal | terminal_finished_wait_updates_from_metadata | fixable | one fixture, not 4 |
| 1.94 | task_list_keys | task_list_unknown_keys_and_help | fixable | split unknown keys, help |
| 1.91 | terminal_layout::terminal_chrome_tabs_close | terminal_claude_chrome_tabs_close | fixable | smaller fixture |
| 1.90 | terminal_layout::terminal_codex_exit | terminal_codex_completed_subagent_calls | fixable | wait_for, not sleep |
| 1.89 | terminal_layout | terminal_background_event_has_no_marker_and_keeps_its_time | fixable | one fixture, not 2<br>split marker, time |
| 1.88 | terminal_layout::terminal_await_output | terminal_background_await_shows_message_without_task_id | fixable | split per start |
| 1.82 | terminal_layout::terminal_codex_exit | terminal_codex_exit_sections | fixable | smaller fixture |
| 1.72 | worktree_dialog_terminal | worktree_confirmation_options_show_directory_and_ellipsis | fixable | split directory, ellipsis |
| 1.71 | terminal_layout::terminal_drafts | terminal_repository_browser_applies_single_clicks | partly fixable | wait_for, not sleep |
| 1.62 | terminal_layout::terminal_agent_messages | terminal_codex_collaboration_navigation | partly fixable | one fixture, not 4<br>split per step |
| 1.59 | terminal_layout::terminal_output_borders | terminal_output_border_double_clicks | fixable | split per border |
| 1.38 | terminal_layout::terminal_task_details_borders | terminal_task_details_border_double_clicks | fixable | split per border |
| 1.36 | terminal_layout::terminal_drafts | terminal_draft_delete_focus_allows_repeated_new_task | fixable | split second new-task round |
| 1.31 | terminal_layout::terminal_heredoc_format | terminal_shell_heredoc_formatting | fixable | one fixture, not 2 |
| 1.20 | terminal_composer | xterm_tmux_keeps_fallbacks_and_dashboard_keys | partly fixable | split fallbacks, dashboard keys |
| 1.18 | web_search_terminal | web_search_details_and_browser_link | fixable | one fixture, not 3<br>split details, link |
| 1.09 | terminal_layout | terminal_task_details_borders_keep_status_and_elapsed_separate | fixable | wait_for, not sleep |
| 1.09 | terminal_layout | terminal_task_cursor_keeps_cell_colors | fixable | wait_for, not sleep |
| 1.07 | terminal_layout | terminal_manager_detach_title | fixable | one fixture, not 2 |
| 1.07 | terminal_layout::terminal_event_detail_width | terminal_preformatted_detail_stays_in_content_column | fixable | smaller fixture |
| 1.06 | terminal_layout::terminal_path_display | terminal_task_details_directory_precedes_title_and_branch | partly fixable | split directory, branch<br>wait_for, not sleep |
| 1.00 | terminal_layout::terminal_scripts | terminal_shell_sed_grep_head_colors | fixable | split sed, grep, head |
| 0.72 | terminal_layout::terminal_event_search | terminal_event_search_finds_trailing_tool_after_unicode | fixable | wait_for, not sleep |
