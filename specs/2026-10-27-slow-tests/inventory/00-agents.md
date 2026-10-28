## Partly fixable

| Run-time (s) | Module | Test | Fixable | How |
|---|---|---|---|---|
| 1.853 | providers::codex::rollout::summary_cache_tests::latest_activity | summaries_match_full_parse_after_each_append | partly | 156 append prefixes; repeated full-file oracle |
| 1.669 | ui::event_list::reading_tests | claude_detail_blocks_are_centered | partly | 11 fixtures at 3 widths |
| 1.639 | app::ui_tests | event_selection_scrolls_to_first_row_across_windows_after_append_at_width_120 | partly | product prepare_event_search cost; minimum height 19 |
| 1.871 | app::ui_tests | event_selection_scrolls_to_first_row_across_windows_after_append_at_width_180 | partly | product prepare_event_search cost; minimum height 19 |
| 1.609 | app::tests | task_event_wheel_then_arrow_keys_reverse_across_reloaded_windows | partly | event panels require height >= 19 |

## Fixable

| Run-time (s) | Module | Test | Fixable | How |
|---|---|---|---|---|
| 1.609 | app::tests | task_event_wheel_then_j_k_keys_reverse_across_reloaded_windows | yes | event panels require height >= 19 |
| 1.570 | app::tests | task_event_wheel_reverses_across_reloaded_windows | yes | event panels require height >= 19 |

## Not fixable

| Run-time (s) | Module | Test | Fixable | How |
|---|---|---|---|---|
| 2.82 (iso 0.45) | ui::tests::focus_rules_tests | rule_8_draft_branch_editor_updates_same_panel_focus | no | contention only; under 1s alone |
