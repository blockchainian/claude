| Run-time (s) | Module | Test | Fixable | How |
|---:|---|---|---|---|
| 2.00 (timed out) | ui::tests::focus_rules_tests | rule_8_draft_branch_editor_updates_same_panel_focus | | |
| 1.00 | state::tests | concurrent_processes_preserve_independent_fields | | |

## tmux probes

Run with `cargo nextest run --run-ignored only`. Probes that hit the 2s timeout were re-run in parallel without it; "failed" means the probe failed after that time.

| Run-time (s) | Module | Test | Fixable | How |
|---:|---|---|---|---|
| 32.94 (failed) | terminal_layout::terminal_drafts | terminal_draft_escape_follows_task_details_and_prompt_focus | | |
| 32.74 (failed) | terminal_layout::terminal_agent_messages | terminal_claude_agent_exchange | | |
| 32.43 (failed) | terminal_layout | terminal_duration_sort_jump_position | | |
| 32.39 (failed) | terminal_layout | terminal_event_metadata_aligns_new_with_clock | | |
| 32.37 (failed) | terminal_layout::terminal_event_scroll | terminal_event_scroll_window_and_boundaries | | |
| 32.23 (failed) | terminal_layout::terminal_drafts | terminal_title_blur_saves_and_unchanged_enter_closes | | |
| 32.22 (failed) | terminal_layout | terminal_event_border_double_clicks | | |
| 32.19 (failed) | terminal_composer | failed_send_shows_its_reason_in_red_in_tmux | | |
| 32.14 (failed) | terminal_layout::terminal_drafts | terminal_task_names_edit_on_task_details_rule | | |
| 32.10 (failed) | terminal_layout::terminal_drafts | terminal_new_task_status_is_capitalized_in_list_and_task_details | | |
| 32.10 (failed) | terminal_layout::terminal_drafts | terminal_draft_name_blur_persists_without_enter | | |
| 32.06 (failed) | terminal_layout | terminal_configuration_version_notices | | |
| 31.64 (failed) | terminal_layout::terminal_task_navigation | terminal_task_navigation_and_opening_keys | | |
| 31.64 (failed) | terminal_layout::terminal_selection_display | terminal_drag_copies_display_lines_and_joins_markdown_wraps | | |
| 31.40 (failed) | terminal_layout::terminal_tool_list | terminal_claude_tool_list_names_groups_and_paths | | |
| 31.11 (failed) | terminal_layout::terminal_task_follow | terminal_task_follow_only_returns_to_active | | |
| 30.86 (failed) | terminal_layout::terminal_task_navigation | terminal_task_border_double_clicks | | |
| 30.85 (failed) | terminal_layout::terminal_task_follow | terminal_pinned_separator_is_centered_and_not_selectable | | |
| 30.69 (failed) | terminal_layout::terminal_window_navigation | terminal_vim_window_navigation_uses_rendered_panels | | |
| 30.67 (failed) | terminal_layout::terminal_transcript | terminal_transcript_modes | | |
| 30.66 (failed) | terminal_layout | terminal_prompt_marker_caret_and_mouse_alignment | | |
| 30.65 (failed) | terminal_layout | terminal_filtered_event_labels_and_typed_prompts | | |
| 30.64 (failed) | terminal_layout::terminal_window_navigation | terminal_active_panel_paging_stays_in_focused_panel | | |
| 30.61 (failed) | terminal_layout::terminal_questions | terminal_question_footer_styles_and_clicks | | |
| 30.51 (failed) | terminal_layout::terminal_prompt_mouse | terminal_prompt_mouse_click_places_caret | | |
| 30.49 (failed) | terminal_layout::terminal_notification | terminal_notification_transcripts_show_only_the_report | | |
| 30.49 (failed) | terminal_layout::terminal_prompt_cache | terminal_prompt_cache_countdown_on_task_details_border | | |
| 30.49 (failed) | terminal_layout | terminal_prompt_newline_entry | | |
| 30.48 (failed) | terminal_layout::terminal_headings | terminal_parallel_tools_have_only_section_titles | | |
| 30.48 (failed) | terminal_layout::terminal_images | terminal_image_path_and_clipboard_are_atomic | | |
| 30.48 (failed) | terminal_layout | terminal_task_notification_shows_summary_without_id_and_missing_file | | |
| 30.47 (failed) | terminal_layout::terminal_headings | terminal_event_section_titles_and_scrolling | | |
| 30.47 (failed) | terminal_layout::terminal_images | terminal_middle_paste_uses_clicked_input | | |
| 30.47 (failed) | terminal_layout::terminal_model_command | terminal_model_command_refreshes_without_an_assistant_reply | | |
| 30.47 (failed) | terminal_layout | terminal_prompt_kill_copies_and_ctrl_y_pastes | | |
| 30.47 (failed) | terminal_layout::terminal_questions | terminal_shared_footer_clicks_and_help_keys | | |
| 30.47 (failed) | terminal_layout::terminal_notification | terminal_background_notifications_shorten_titles_and_color_exit_numbers | | |
| 30.46 (failed) | terminal_layout::terminal_mermaid | terminal_mermaid_diagrams_and_fallback | | |
| 30.46 (failed) | terminal_layout::terminal_task_follow | terminal_status_tabs_filter_children_and_preserve_ancestor_status | | |
| 30.45 (failed) | terminal_layout | terminal_weekly_usage_on_task_header | | |
| 25.88 | agents::providers::codex::app_server::tests | delete_real_codex_sessions | | |
| 21.23 | terminal_layout::terminal_drafts | terminal_draft_defaults_and_inherited_choices | | |
| 21.01 (failed) | terminal_theme_colors | terminal_theme_colors_follow_bundled_and_custom_themes | | |
| 17.35 | terminal_layout::terminal_scripts | terminal_shell_file_read_colors | | |
| 15.48 | terminal_layout | terminal_layout_tracks_width | | |
| 15.34 | agents::providers::codex::app_server::tests | reject_real_codex_question_interrupts_turn | | |
| 14.55 | terminal_layout::terminal_output_follow | terminal_output_files_follow_appends_and_preserve_reading_position | | |
| 13.42 (failed) | agents::providers::codex::app_server::tests | stop_and_archive_real_codex_thread | | |
| 12.38 | agents::providers::codex::terminal_live_tests | idle_terminal_archives_real_codex_thread_without_hiding_it | | |
| 12.31 | agents::providers::claude::rename_live_tests | rename_real_socket_session | | |
| 11.10 | agents::providers::claude::attach_live_tests | real_terminal_panel_clears_left_input | | |
| 10.81 | agents::providers::claude::settings_tests | settings_real_claude_session | | |
| 10.74 (failed) | agents::providers::claude::attach_live_tests | real_attach_prompt_paste_busy_steer_interrupt_model | | |
| 9.12 | agents::providers::codex::app_server::tests | rename_real_codex_thread | | |
| 9.08 | terminal_composer | readline_words_match_in_composer | | |
| 8.44 (failed) | agents::providers::codex::settings_tests | settings_real_codex_thread | | |
| 8.24 | agents::providers::codex::app_server::tests | approval_status_marks_a_live_codex_session | | |
| 8.14 (failed) | agents::providers::codex::ask_live_tests | answer_real_codex_question_in_dashboard | | |
| 7.88 (failed) | agents::providers::codex::send_tests | send_real_codex_thread | | |
| 7.50 | terminal_layout::terminal_drafts | terminal_draft_wizard_focus_order_and_clicks | | |
| 6.91 | terminal_layout::terminal_event_scroll | terminal_send_preserves_reading_position | | |
| 6.48 (failed) | agents::providers::claude::attach_live_tests::subagent_live_tests | real_dashboard_prompt_reaches_running_subagent_and_restores_main | | |
| 5.80 (failed) | agents::providers::claude::daemon_socket::live_tests | real_background_input_client_and_dashboard | | |
| 5.44 | terminal_layout | terminal_running_tool_animates_without_input | | |
| 4.42 | agents::app::session_terminal::tests | real_woken_terminal_closes_back_to_inactive | | |
| 4.04 | agents::providers::claude::attach::tests | real_model_command_without_confirmation | | |
| 3.93 | terminal_layout | terminal_context_usage_on_task_details_border | | |
| 3.91 | terminal_layout | terminal_task_details_section_positions | | |
| 3.27 (failed) | terminal_layout::terminal_link_overlays | terminal_confirmation_border_covers_wide_text | | |
| 3.25 (failed) | agents::ui::tests | task_header_weekly_usage_live_accounts | | |
| 3.18 | terminal_layout::terminal_model_families | terminal_model_families_keep_headers_out_of_selection | | |
| 2.93 | agents::ui::event_list::event_markdown | markdown_frame_timing | | |
| 2.87 | terminal_composer | enhanced_keys_newlines_and_vertical_caret_in_tmux | | |
| 2.87 | terminal_layout | terminal_update_click_selects_source_row | | |
| 2.45 | terminal_layout | terminal_inactive_elapsed_stays_at_last_record | | |
| 2.44 (failed) | terminal_layout::terminal_questions | terminal_ask_questions_render_real_rejection_and_answer | | |
| 2.00 | spawn_agent_terminal | terminal_spawn_agent_claude_receipt_and_model | | |
| 1.95 | terminal_layout | terminal_branch_name_edits_in_task_details_and_preserves_input | | |
| 1.95 | finished_subagent_terminal | terminal_finished_wait_updates_from_metadata | | |
| 1.94 | task_list_keys | task_list_unknown_keys_and_help | | |
| 1.91 | terminal_layout::terminal_chrome_tabs_close | terminal_claude_chrome_tabs_close | | |
| 1.90 | terminal_layout::terminal_codex_exit | terminal_codex_completed_subagent_calls | | |
| 1.89 | terminal_layout | terminal_background_event_has_no_marker_and_keeps_its_time | | |
| 1.88 | terminal_layout::terminal_await_output | terminal_background_await_shows_message_without_task_id | | |
| 1.82 | terminal_layout::terminal_codex_exit | terminal_codex_exit_sections | | |
| 1.72 | worktree_dialog_terminal | worktree_confirmation_options_show_directory_and_ellipsis | | |
| 1.71 | terminal_layout::terminal_drafts | terminal_repository_browser_applies_single_clicks | | |
| 1.62 | terminal_layout::terminal_agent_messages | terminal_codex_collaboration_navigation | | |
| 1.59 | terminal_layout::terminal_output_borders | terminal_output_border_double_clicks | | |
| 1.38 | terminal_layout::terminal_task_details_borders | terminal_task_details_border_double_clicks | | |
| 1.36 | terminal_layout::terminal_drafts | terminal_draft_delete_focus_allows_repeated_new_task | | |
| 1.31 | terminal_layout::terminal_heredoc_format | terminal_shell_heredoc_formatting | | |
| 1.20 | terminal_composer | xterm_tmux_keeps_fallbacks_and_dashboard_keys | | |
| 1.18 | web_search_terminal | web_search_details_and_browser_link | | |
| 1.09 | terminal_layout | terminal_task_details_borders_keep_status_and_elapsed_separate | | |
| 1.09 | terminal_layout | terminal_task_cursor_keeps_cell_colors | | |
| 1.07 | terminal_layout | terminal_manager_detach_title | | |
| 1.07 | terminal_layout::terminal_event_detail_width | terminal_preformatted_detail_stays_in_content_column | | |
| 1.06 | terminal_layout::terminal_path_display | terminal_task_details_directory_precedes_title_and_branch | | |
| 1.00 | terminal_layout::terminal_scripts | terminal_shell_sed_grep_head_colors | | |
| 0.72 | terminal_layout::terminal_event_search | terminal_event_search_finds_trailing_tool_after_unicode | | |
