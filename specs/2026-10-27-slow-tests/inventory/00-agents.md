| Run-time (s) | Module | Test | Fixable | How |
|---:|---|---|---|---|
| 2.55 | providers::codex::rollout::summary_cache_tests::latest_activity | summaries_match_full_parse_after_each_append | fixable | 156 行 fixture 去掉重复 wait/exec 和无关元数据 |
| 2.32 | ui::event_list::reading_tests | claude_detail_blocks_are_centered | fixable | 按详情类型拆分<br>删未用的 Codex fixture<br>缩短长代码块 |
| 2.09 | app::tests | task_event_wheel_then_j_k_keys_reverse_across_reloaded_windows | fixable | fixture 正文缩成编号短句 |
| 2.08 | app::tests | task_event_wheel_then_arrow_keys_reverse_across_reloaded_windows | fixable | fixture 正文缩成编号短句 |
| 1.99 | app::tests | task_event_wheel_reverses_across_reloaded_windows | fixable | fixture 正文缩成编号短句 |
| 1.84 | app::ui_tests | event_selection_scrolls_to_first_row_across_windows_after_append_at_width_180 | fixable | 40 条缩到刚够跨窗<br>正文只留编号 |
| 1.39 | app::ui_tests | event_selection_scrolls_to_first_row_across_windows_after_append_at_width_120 | fixable | 40 条缩到刚够跨窗<br>正文只留编号 |
| 1.13 | ui::tests | selected_event_row_preserves_foreground_and_adds_bold | fixable | 拆成普通行、搜索行<br>组合渲染 576 次改成单元测试<br>只用所需 Tool，缩画布 |
| 1.12 | ui::event_list::reading_tests | diff_backgrounds_preserve_text_positions | fixable | 250 行画布缩到刚够<br>只留 Edit、FileChange 记录 |
| 1.08 | app::tests | task_event_search_loads_all_caches_matches_and_refreshes_growth | fixable | 拆成搜索滚动、缓存增长<br>transcript 20 份缩到 5 份<br>滚轮 30 次减到 6 次 |
