## Partly fixable

| Run-time (s) | Module | Test | Fixable | How |
|---|---|---|---|---|
| 8.86 (iso 5.73) | providers::codex::rollout::summary_cache_tests | summaries_match_full_parse_after_each_append | partly | split per fixture<br>truncate long strings in the 536 KB fixture, keep it over 64 KB |
| 3.42 (iso 1.97) | ui::tests::active_panel_tests | active_panel_task_details_navigation_scrolls_only_its_content | partly | split per panel<br>fewer events in panel_fixture |
| 1.669 | ui::event_list::reading_tests | claude_detail_blocks_are_centered | partly | 11 fixtures at 3 widths |
| 1.639 | app::ui_tests | event_selection_scrolls_to_first_row_across_windows_after_append_at_width_120 | partly | product prepare_event_search cost; minimum height 19 |
| 1.871 | app::ui_tests | event_selection_scrolls_to_first_row_across_windows_after_append_at_width_180 | partly | product prepare_event_search cost; minimum height 19 |
| 2.61 (iso 1.46) | ui::tests::active_panel_tests | mouse_activation_every_event_routes_subsequent_paging | partly | fewer events in panel_fixture<br>split per width |
| 2.08 (iso 1.52) | providers::codex::rollout::summary_cache_tests | summaries_parse_each_byte_once_across_appends | partly | split per fixture<br>truncated copy of the big fixture |
| 2.06 (iso 1.14) | ui::tests::active_panel_tests | mouse_activation_drag_keeps_the_pressed_panel | partly | fewer events in panel_fixture<br>split per width |
| 2.05 (iso 1.51) | app::tests | task_event_wheel_then_arrow_keys_reverse_across_reloaded_windows | partly | smaller area height |
| 1.56 (iso 1.27) | app::tests | new_indicator_loads_latest_event_after_top_border | partly | smaller area, fewer transcript copies |
| 1.21 (iso 0.75) | ui::tests::active_panel_tests | active_panel_question_keys_require_prompt_focus | partly | split per width<br>fewer events in panel_fixture |
| 1.19 (iso 0.98) | app::tests | bottom_border_loads_latest_event_after_top_border | partly | smaller area<br>drop duplicated top-border asserts |
| 1.15 (iso 0.74) | ui::tests::active_panel_tests | active_panel_unsupported_task_keys_do_not_fall_through | partly | split per panel<br>fewer events in panel_fixture |
| 1.13 (iso 0.72) | ui::tests::active_panel_tests | active_panel_detail_bottom_forward_keys_do_not_navigate_lists | partly | split per panel<br>fewer events in panel_fixture |
| 1.07 (iso 0.81) | app::tests | top_border_loads_first_event_of_whole_event | partly | smaller area, fewer transcript copies |
| 1.03 (iso 0.62) | ui::tests::active_panel_tests | active_panel_short_event_detail_steps_between_events | partly | split per width<br>fewer events in panel_fixture |

## Fixable

| Run-time (s) | Module | Test | Fixable | How |
|---|---|---|---|---|
| 2.08 (iso 1.54) | providers::codex::rollout::summary_cache_tests | summaries_do_not_reparse_unchanged_files | yes | redundant: move its one assert, delete it |
| 2.05 (iso 1.54) | app::tests | task_event_wheel_then_j_k_keys_reverse_across_reloaded_windows | yes | smaller area height |
| 2.00 (iso 1.48) | app::tests | task_event_wheel_reverses_across_reloaded_windows | yes | smaller area height |
| 1.95 (iso 1.16) | ui::tests::active_panel_tests | mouse_activation_double_click_keeps_the_pressed_panel | yes | fewer events in panel_fixture |
| 1.70 (iso 1.06) | ui::tests::active_panel_tests | active_panel_event_detail_bottom_steps_to_next_event | yes | fewer events in panel_fixture |
| 1.48 (iso 0.92) | ui::tests::active_panel_tests | active_panel_detail_top_backward_keys_do_not_navigate_lists | yes | split per panel<br>fewer events in panel_fixture |

## Not fixable

| Run-time (s) | Module | Test | Fixable | How |
|---|---|---|---|---|
| 2.82 (iso 0.45) | ui::tests::focus_rules_tests | rule_8_draft_branch_editor_updates_same_panel_focus | no | contention only; under 1s alone |
