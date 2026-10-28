| Run-time (s) | Module | Test | Fixable | How |
|---:|---|---|---|---|
| 2.55 | providers::codex::rollout::summary_cache_tests::latest_activity | summaries_match_full_parse_after_each_append | fixable | 156 行缩到约 20 行<br>仍超过 64 KiB |
| 2.32 | ui::event_list::reading_tests | claude_detail_blocks_are_centered | fixable | 每个 Claude fixture 一个测试<br>不建 Codex 数据 |
| 2.09 | app::tests | task_event_wheel_then_j_k_keys_reverse_across_reloaded_windows | fixable | 75 条缩到 27 条<br>滚 39 次减到 15 次 |
| 2.08 | app::tests | task_event_wheel_then_arrow_keys_reverse_across_reloaded_windows | fixable | 75 条缩到 27 条<br>滚 39 次减到 15 次 |
| 1.99 | app::tests | task_event_wheel_reverses_across_reloaded_windows | fixable | 75 条缩到 27 条<br>滚 39 次减到 15 次 |
| 1.84 | app::ui_tests | event_selection_scrolls_to_first_row_across_windows_after_append_at_width_180 | fixable | 40 条缩到 20 条<br>走 28 步减到 7 步 |
| 1.39 | app::ui_tests | event_selection_scrolls_to_first_row_across_windows_after_append_at_width_120 | fixable | 40 条缩到 20 条<br>走 28 步减到 7 步 |
| 1.13 | ui::tests | selected_event_row_preserves_foreground_and_adds_bold | fixable | 组合 288 次拆成按维度 17 次 |
| 1.12 | ui::event_list::reading_tests | diff_backgrounds_preserve_text_positions | fixable | 250 行画布缩到最高 diff 的高度 |
| 1.08 | app::tests | task_event_search_loads_all_caches_matches_and_refreshes_growth | fixable | 拆成三个测试<br>20 份缩到 3 份 |
