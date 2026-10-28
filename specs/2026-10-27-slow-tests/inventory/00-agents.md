## Partly fixable

| Run-time (s) | Module | Test | Fixable | How |
|---|---|---|---|---|
| 8.86 (iso 5.73) | providers::codex::rollout::summary_cache_tests | summaries_match_full_parse_after_each_append | partly | split per fixture<br>truncate long strings in the 536 KB fixture, keep it over 64 KB |
| 3.42 (iso 1.97) | ui::tests::active_panel_tests | active_panel_task_details_navigation_scrolls_only_its_content | partly | split per panel<br>fewer events in panel_fixture |
| 3.30 (iso 1.92) | ui::event_list::reading_tests | provider_detail_blocks_are_centered | partly | split claude and codex fixtures |
| 3.11 (iso 2.23) | app::ui_tests | event_selection_scrolls_to_first_row_across_windows_after_append | partly | split per width<br>rest is product search-index cost |
| 2.61 (iso 1.46) | ui::tests::active_panel_tests | mouse_activation_every_event_routes_subsequent_paging | partly | fewer events in panel_fixture<br>split per width |
| 2.08 (iso 1.52) | providers::codex::rollout::summary_cache_tests | summaries_parse_each_byte_once_across_appends | partly | split per fixture<br>truncated copy of the big fixture |
| 2.06 (iso 1.14) | ui::tests::active_panel_tests | mouse_activation_drag_keeps_the_pressed_panel | partly | fewer events in panel_fixture<br>split per width |
| 2.05 (iso 1.51) | app::tests | task_event_wheel_then_arrow_keys_reverse_across_reloaded_windows | partly | smaller area height |
| 1.57 (iso 0.86) | ui::render::owner_label_tests | task_labels_follow_visible_owner_and_borrow_only_visible_list_edges | partly | one fixture per width, reset zoom<br>lower area height |
| 1.56 (iso 1.27) | app::tests | new_indicator_loads_latest_event_after_top_border | partly | smaller area, fewer transcript copies |
| 1.44 (iso 0.71) | ui::tests | selected_event_row_preserves_foreground_and_adds_bold | partly | one fixture per width and theme |
| 1.37 (iso 0.71) | ui::render::panel_frame_tests | frame_labels_restore_after_every_zoom | partly | one fixture per width<br>lower area height |
| 1.28 (iso 0.83) | ui::tests::window_navigation_tests | panel_zoom_collapsed_panels_keep_frame_labels | partly | height 45, not 110<br>one fixture per width |
| 1.22 (iso 0.62) | ui::render::owner_label_tests | frame_joints_exist_only_at_visible_column_separators | partly | one fixture per width<br>lower area height |
| 1.21 (iso 0.75) | ui::tests::active_panel_tests | active_panel_question_keys_require_prompt_focus | partly | split per width<br>fewer events in panel_fixture |
| 1.19 (iso 0.98) | app::tests | bottom_border_loads_latest_event_after_top_border | partly | smaller area<br>drop duplicated top-border asserts |
| 1.15 (iso 0.74) | ui::tests::active_panel_tests | active_panel_unsupported_task_keys_do_not_fall_through | partly | split per panel<br>fewer events in panel_fixture |
| 1.13 (iso 0.72) | ui::tests::active_panel_tests | active_panel_detail_bottom_forward_keys_do_not_navigate_lists | partly | split per panel<br>fewer events in panel_fixture |
| 1.07 (iso 0.81) | app::tests | top_border_loads_first_event_of_whole_event | partly | smaller area, fewer transcript copies |
| 1.03 (iso 0.62) | ui::tests::active_panel_tests | active_panel_short_event_detail_steps_between_events | partly | split per width<br>fewer events in panel_fixture |

## Fixable

| Run-time (s) | Module | Test | Fixable | How |
|---|---|---|---|---|
| 6.22 (iso 3.54) | ui::event_list::reading_tests | diff_backgrounds_preserve_text_positions | yes | render only diff-producing events |
| 3.80 (iso 2.19) | ui::event_list::reading_tests | diff_backgrounds_reach_terminal_edges | yes | render only diff-producing events |
| 2.08 (iso 1.54) | providers::codex::rollout::summary_cache_tests | summaries_do_not_reparse_unchanged_files | yes | redundant: move its one assert, delete it |
| 2.05 (iso 1.54) | app::tests | task_event_wheel_then_j_k_keys_reverse_across_reloaded_windows | yes | smaller area height |
| 2.00 (iso 1.48) | app::tests | task_event_wheel_reverses_across_reloaded_windows | yes | smaller area height |
| 1.95 (iso 1.16) | ui::tests::active_panel_tests | mouse_activation_double_click_keeps_the_pressed_panel | yes | fewer events in panel_fixture |
| 1.70 (iso 1.06) | ui::tests::active_panel_tests | active_panel_event_detail_bottom_steps_to_next_event | yes | fewer events in panel_fixture |
| 1.48 (iso 0.92) | ui::tests::active_panel_tests | active_panel_detail_top_backward_keys_do_not_navigate_lists | yes | split per panel<br>fewer events in panel_fixture |
| 1.36 (iso 0.89) | ui::event_scroll_tests | event_scroll_chinese_json_round_trip | yes | repeat text 10x, not 80x<br>test a few heights, not all |
| 1.21 (iso 1.16) | app::session_terminal::tests | terminal_cleanup_waits_for_detach_off_the_ui_thread_on_close_and_switch | yes | child exits after detach, no 500ms timeout<br>split close and switch |
| 1.11 (iso 0.84) | app::ui_tests | medium_and_large_event_panels_scroll_and_copy_independently | yes | split per layout |
| 1.08 (iso 0.69) | ui::event_messages_tests | event_filter_follows_its_panel_through_zoom | yes | split per panel<br>one fixture per panel |
| 1.08 (iso 0.61) | ui::inline_images::tests | inline_images_evicts_least_recently_drawn_image_at_byte_limit | yes | opt-level 2 for image crates in dev profile |
| 1.03 (iso 0.49) | ui::draft_flow_tests | branch_and_repository_selection_orders_create_in_selected_repository | yes | split per order<br>one git repo per test |
| 1.01 (iso 0.57) | ui::tests::window_navigation_tests | hidden_focus_resize_recovers_and_does_not_return_on_growth | yes | split per width and source |

## Not fixable

| Run-time (s) | Module | Test | Fixable | How |
|---|---|---|---|---|
| 2.82 (iso 0.45) | ui::tests::focus_rules_tests | rule_8_draft_branch_editor_updates_same_panel_focus | no | contention only; under 1s alone |
