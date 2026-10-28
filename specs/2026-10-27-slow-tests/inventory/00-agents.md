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

## How

### task_event_wheel_then_j_k_keys_reverse_across_reloaded_windows

在 task_event_navigation_reverses_across_reloaded_windows 中先缩短 payload 正文，再交给 transcript_copies，保留编号、通知、跨窗往返和原有断言。

```rust
// 现在
let payload = std::fs::read_to_string(&path).unwrap();
// 改成
let payload = std::fs::read_to_string(&path).unwrap().lines()
    .map(|line| {
        let mut record: serde_json::Value = serde_json::from_str(line).unwrap();
        if let Some(content) = record.pointer_mut("/message/content") {
            if content.is_string() { *content = "User".into(); } else {
                let text = content.as_array_mut().unwrap().iter_mut()
                    .find(|block| block["type"] == "text").unwrap();
                text["text"] = "Assistant".into();
            }
        }
        format!("{record}\n")
    }).collect::<String>();
```

### task_event_wheel_then_arrow_keys_reverse_across_reloaded_windows

同 `task_event_wheel_then_j_k_keys_reverse_across_reloaded_windows`。

### task_event_wheel_reverses_across_reloaded_windows

同 `task_event_wheel_then_j_k_keys_reverse_across_reloaded_windows`。

### event_selection_scrolls_to_first_row_across_windows_after_append_at_width_180

在 EventSelectionFixture::after_wheel_burst 中去掉重复的长正文，保留 40 条事件、追加逻辑及所有逐行选中和首行断言。

```rust
// 现在
let body = record["message"]["content"].as_str().unwrap().to_owned();
record["message"]["content"] = format!("event-{index:02} {body}").into();
// 改成
let body = String::new();
record["message"]["content"] = format!("event-{index:02} {body}").into();
```

### event_selection_scrolls_to_first_row_across_windows_after_append_at_width_120

同 `event_selection_scrolls_to_first_row_across_windows_after_append_at_width_180`。

### claude_detail_blocks_are_centered

按 Claude fixture 拆成独立测试，各自调用原断言函数，保留全部 fixture 和宽度覆盖，同时避免加载未使用的 Codex 数据。

```rust
// 现在
let (entries, _) = provider_detail_events();
assert_provider_detail_blocks_centered(entries);
// 改成
// 每份原有 Claude fixture 各放一个测试，这里展示其中一个。
let entries = events(include_str!("../../tests/fixtures/claude-tool-calls.jsonl"));
assert_provider_detail_blocks_centered(entries);
```

### selected_event_row_preserves_foreground_and_adds_bold

用已有 event_fixture 中的 Tool 事件替代读取整份 transcript，保留后面的主题、宽度、种类、搜索和两行选中断言。

```rust
// 现在
let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("tests/fixtures/claude-tool-calls.jsonl");
let event = crate::providers::claude::event_reader::tail_session(&source, 200, true)
    .unwrap().into_iter()
    .find(|event| event.kind == Kind::Tool).unwrap();
// 改成
let (view, _) = event_fixture();
let event = view.event_panel.into_iter()
    .find(|event| event.kind == Kind::Tool).unwrap();
// 后续仍覆盖 body、清空 calls / paths，并运行全部原断言。
```

### diff_backgrounds_preserve_text_positions

参考缓冲区只分配实际详情面板，保持原来的渲染矩形、全部可见行和逐格比较不变。

```rust
// 现在
let panel = view.center_layout(area).event_detail;
let reference = View::new(Grouping::Project, false);
let mut before = Buffer::empty(area);
// 改成
let panel = view.center_layout(area).event_detail;
let reference = View::new(Grouping::Project, false);
let mut before = Buffer::empty(panel);
// render_event_detail_content 仍传 viewport.event_detail。
// 原有 panel.x..panel.right() 范围的逐格断言全部保留。
```

### summaries_match_full_parse_after_each_append

精简同一个 JSONL fixture 里的重复 wait/exec 轮次和无关元数据，保留各类状态、调用配对及逐行断言；Rust 调用不变，仍要求跨过 64 KiB。

```rust
// 现在：fixture 有 156 行，包含重复轮次
summary_fixture!(
    latest_activity,
    "../../../tests/fixtures/codex-summary-activity.jsonl"
);
// 改成：精简同一 fixture，保留原有调用和跨页断言
const _: () = assert!(
    include_str!("../../../tests/fixtures/codex-summary-activity.jsonl").len() as u64 > CHUNK
);
summary_fixture!(
    latest_activity,
    "../../../tests/fixtures/codex-summary-activity.jsonl"
);
```

### task_event_search_loads_all_caches_matches_and_refreshes_growth

把两处 transcript 副本都从 20 份缩到 5 份，滚轮从 30 次缩到 6 次，并同步调整计数，保留其余断言。

```rust
// 现在
replace_transcript(&path, transcript_copies(&payload, 20, 0));
assert_eq!(app.view.matching_event_count(), 20);
for _ in 0..30 { scroll_event(&mut app, crossterm::event::MouseEventKind::ScrollUp); }
std::fs::write(&path, transcript_copies(&payload.replace("Codex", "other"), 20, 0)).unwrap();
assert_eq!(app.view.matching_event_count(), 21);
assert_eq!(app.view.event_panel.len(), 41);
// 改成
replace_transcript(&path, transcript_copies(&payload, 5, 0));
assert_eq!(app.view.matching_event_count(), 5);
for _ in 0..6 { scroll_event(&mut app, crossterm::event::MouseEventKind::ScrollUp); }
std::fs::write(&path, transcript_copies(&payload.replace("Codex", "other"), 5, 0)).unwrap();
assert_eq!(app.view.matching_event_count(), 6);
assert_eq!(app.view.event_panel.len(), 11);
```
