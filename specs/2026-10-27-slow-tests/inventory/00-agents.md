# agents tests slower than 1s

From one full `cargo nextest run` in `agents/` on 2026-10-27, branch `fix-slow-tests`: 2638 tests in parallel, 16.4s wall. Times are per test while running in parallel.

| # | Seconds | Test |
|---:|---:|---|
| 1 | 8.86 | providers::codex::rollout::summary_cache_tests::summaries_match_full_parse_after_each_append |
| 2 | 6.22 | ui::event_list::reading_tests::diff_backgrounds_preserve_text_positions |
| 3 | 3.80 | ui::event_list::reading_tests::diff_backgrounds_reach_terminal_edges |
| 4 | 3.42 | ui::tests::active_panel_tests::active_panel_task_details_navigation_scrolls_only_its_content |
| 5 | 3.30 | ui::event_list::reading_tests::provider_detail_blocks_are_centered |
| 6 | 3.11 | app::ui_tests::event_selection_scrolls_to_first_row_across_windows_after_append |
| 7 | 2.82 | ui::tests::focus_rules_tests::rule_8_draft_branch_editor_updates_same_panel_focus |
| 8 | 2.61 | ui::tests::active_panel_tests::mouse_activation_every_event_routes_subsequent_paging |
| 9 | 2.08 | providers::codex::rollout::summary_cache_tests::summaries_parse_each_byte_once_across_appends |
| 10 | 2.08 | providers::codex::rollout::summary_cache_tests::summaries_do_not_reparse_unchanged_files |
| 11 | 2.06 | ui::tests::active_panel_tests::mouse_activation_drag_keeps_the_pressed_panel |
| 12 | 2.05 | app::tests::task_event_wheel_then_arrow_keys_reverse_across_reloaded_windows |
| 13 | 2.05 | app::tests::task_event_wheel_then_j_k_keys_reverse_across_reloaded_windows |
| 14 | 2.00 | app::tests::task_event_wheel_reverses_across_reloaded_windows |
| 15 | 1.95 | ui::tests::active_panel_tests::mouse_activation_double_click_keeps_the_pressed_panel |
| 16 | 1.70 | ui::tests::active_panel_tests::active_panel_event_detail_bottom_steps_to_next_event |
| 17 | 1.57 | ui::render::owner_label_tests::task_labels_follow_visible_owner_and_borrow_only_visible_list_edges |
| 18 | 1.56 | app::tests::new_indicator_loads_latest_event_after_top_border |
| 19 | 1.48 | ui::tests::active_panel_tests::active_panel_detail_top_backward_keys_do_not_navigate_lists |
| 20 | 1.44 | ui::tests::selected_event_row_preserves_foreground_and_adds_bold |
| 21 | 1.37 | ui::render::panel_frame_tests::frame_labels_restore_after_every_zoom |
| 22 | 1.36 | ui::event_scroll_tests::event_scroll_chinese_json_round_trip |
| 23 | 1.28 | ui::tests::window_navigation_tests::panel_zoom_collapsed_panels_keep_frame_labels |
| 24 | 1.22 | ui::render::owner_label_tests::frame_joints_exist_only_at_visible_column_separators |
| 25 | 1.21 | ui::tests::active_panel_tests::active_panel_question_keys_require_prompt_focus |
| 26 | 1.21 | app::session_terminal::tests::terminal_cleanup_waits_for_detach_off_the_ui_thread_on_close_and_switch |
| 27 | 1.19 | app::tests::bottom_border_loads_latest_event_after_top_border |
| 28 | 1.15 | ui::tests::active_panel_tests::active_panel_unsupported_task_keys_do_not_fall_through |
| 29 | 1.13 | ui::tests::active_panel_tests::active_panel_detail_bottom_forward_keys_do_not_navigate_lists |
| 30 | 1.11 | app::ui_tests::medium_and_large_event_panels_scroll_and_copy_independently |
| 31 | 1.08 | ui::event_messages_tests::event_filter_follows_its_panel_through_zoom |
| 32 | 1.08 | ui::inline_images::tests::inline_images_evicts_least_recently_drawn_image_at_byte_limit |
| 33 | 1.07 | app::tests::top_border_loads_first_event_of_whole_event |
| 34 | 1.03 | ui::tests::active_panel_tests::active_panel_short_event_detail_steps_between_events |
| 35 | 1.03 | ui::draft_flow_tests::branch_and_repository_selection_orders_create_in_selected_repository |
| 36 | 1.01 | ui::tests::window_navigation_tests::hidden_focus_resize_recovers_and_does_not_return_on_growth |
