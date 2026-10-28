## Partly fixable

| Run-time (s) | Module | Test | Fixable | How |
|---|---|---|---|---|
| 2.46 (iso 1.85) | providers::codex::rollout::summary_cache_tests::latest_activity | summaries_match_full_parse_after_each_append | partly | full-file oracle at each of 156 appends |
| 2.42 (iso 1.61) | app::tests | task_event_wheel_then_arrow_keys_reverse_across_reloaded_windows | partly | panels need height >= 19<br>product render cost per step |
| 2.25 (iso 1.67) | ui::event_list::reading_tests | claude_detail_blocks_are_centered | partly | 11 fixtures at 3 widths |
| 2.22 (iso 1.61) | app::tests | task_event_wheel_then_j_k_keys_reverse_across_reloaded_windows | partly | panels need height >= 19<br>product render cost per step |
| 2.14 (iso 1.57) | app::tests | task_event_wheel_reverses_across_reloaded_windows | partly | panels need height >= 19<br>product render cost per step |
| 1.63 (iso 1.87) | app::ui_tests | event_selection_scrolls_to_first_row_across_windows_after_append_at_width_180 | partly | product prepare_event_search cost |
| 1.34 (iso 1.64) | app::ui_tests | event_selection_scrolls_to_first_row_across_windows_after_append_at_width_120 | partly | product prepare_event_search cost |

## Not fixable

| Run-time (s) | Module | Test | Fixable | How |
|---|---|---|---|---|
| 2.73 (iso 0.46) | ui::tests::focus_rules_tests | rule_8_draft_branch_editor_updates_same_panel_focus | no | contention only |
| 1.17 (iso 0.77) | app::tests | task_event_search_loads_all_caches_matches_and_refreshes_growth | no | contention only |
| 1.12 (iso 0.58) | ui::tests | selected_event_row_preserves_foreground_and_adds_bold | no | contention only |
| 1.10 (iso 0.75) | ui::event_list::reading_tests | diff_backgrounds_preserve_text_positions | no | contention only |
