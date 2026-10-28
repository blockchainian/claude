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

## How

Window sizes measured in a copy of the crate: `event_list_read_count` grows the window by `rows` each time the selection reaches scroll 0 with `has_older`. In `task_event_navigation_reverses_across_reloaded_windows` at `Rect(0,0,150,19)` `visible = 8`, so `copies = 17`, the transcript holds 2*(20+17)+1 = 75 events, and the 39 wheel scrolls reload the window five times (8 → 16 → 24 → 32 → 40 → 48). In `after_wheel_burst` at height 19 `event_read_count = 4`; the 40-record fixture loads 17 after the burst and the 28-step walk reloads six times (17 → 21 → … → 41). Alone, the two run in 1.46s and 0.99s.

### summaries_match_full_parse_after_each_append

The test is quadratic: each of the 156 appended lines runs `expected()` which re-parses the whole file, so cost is `lines × bytes / 2` ≈ 10 MB of JSON. The fixture must stay above `CHUNK` (64 KiB) for tail expansion, so cut lines rather than bytes: keep one record per `(type, payload.type)` pair that the summary reads, and pad the file over 64 KiB with a single large `function_call_output` record. That keeps every record kind and the chunk boundary but drops the line count from 156 to about 20, cutting the parse volume roughly 8x.

```rust
// now (fixture, 156 lines, 132 KiB; test code unchanged)
// change to: regenerate codex-summary-activity.jsonl in its .md script with
//   one record per distinct (type, payload.type) in first-seen order,
//   plus the first function_call_output padded to > 64 KiB
// The compile-time `assert!(len > CHUNK)` still guards the boundary.
```

### claude_detail_blocks_are_centered

The test parses all eleven Claude fixtures and, through `provider_detail_events`, also builds the whole Codex fixture set it never uses, then renders every event at three widths. Split into one test per Claude fixture that parses only its own file; `codex_detail_blocks_are_centered` keeps the Codex half.

```rust
// now
let (entries, _) = provider_detail_events();
assert_provider_detail_blocks_centered(entries);
// change to (one #[test] per fixture, e.g.)
assert_provider_detail_blocks_centered(events(include_str!(
    "../../tests/fixtures/claude-tool-details.jsonl"
)));
```

### task_event_wheel_then_j_k_keys_reverse_across_reloaded_windows

Same change as `task_event_wheel_reverses_across_reloaded_windows`.

### task_event_wheel_then_arrow_keys_reverse_across_reloaded_windows

Same change as `task_event_wheel_reverses_across_reloaded_windows`; the shared helper takes the smaller counts.

### task_event_wheel_reverses_across_reloaded_windows

The behaviour is: scrolling older reloads the window, and six steps back must pass the notification and land on the same detail. That needs one reload above the notification and one window of older copies behind it, not 75 events. Keep the structure but write `visible` copies before the notification and `visible / 2 + 1` after it: 16 + 1 + 10 = 27 events, 15 scrolls, one reload (8 → 16) still asserted by `len() > loaded`. The `"Transcript copy 20"` assertion becomes `"Transcript copy {first}"`.

```rust
// now
transcript_copies(&payload, 20, 0),
include_str!(".../claude-task-notifications.jsonl"),
transcript_copies(&payload, copies, 20)
// change to
let first = visible;              // one window of older copies behind the notification
let copies = visible / 2 + 1;     // 10 events: first window plus two past the reload
transcript_copies(&payload, first, 0),
include_str!(".../claude-task-notifications.jsonl"),
transcript_copies(&payload, copies, first)
// and assert `after.contains(&format!("Transcript copy {first}"))`
```

Prototype: passes in 0.79s alone, down from 1.46s alone. The parallel-run figure of 1.99s was 1.36x the solo time, so expect about 1.0s in the full run; the remaining cost is one full `view.render` per scroll inside `wheel`, which the test needs.

### event_selection_scrolls_to_first_row_across_windows_after_append_at_width_180

Same change as the width-120 variant; the fixture is shared.

### event_selection_scrolls_to_first_row_across_windows_after_append_at_width_120

`after_wheel_burst` writes 40 records and the test walks 28 scrolls back to `event-00`, reloading six 4-row windows; the behaviour under test is "after an append, scrolling older still lands on the first row of each reloaded window", which needs one reload, not six. Cut the fixture to 20 records: the burst of 12 from `event-19` selects `event-07`, the window after the burst holds 17, and the walk from `event-06` to `event-00` reloads once (17 → 21). The sibling tests on the same fixture change their labels from `event-39`/`event-27`/`event-40` to `event-19`/`event-07`/`event-20`.

```rust
// now
for index in 0..40 {
...
for expected in (0..27).rev() {
// change to
for index in 0..20 {          // burst of 12 + one 4-row reload + the first rows
...
for expected in (0..7).rev() {
```

Prototype: passes in 0.39s alone, down from 0.99s; the two sibling tests on the fixture pass with the relabelled events in 0.28s and 0.38s.

### selected_event_row_preserves_foreground_and_adds_bold

The test is a full product of 2 themes × 4 widths × 9 kinds × 2 search states × 2 rows, 288 renders of a whole `event_fixture` view, to check one invariant: the selected row keeps its foreground and gains bold. The axes are independent, so test each axis once against a fixed baseline: 9 kinds at width 150, dark, no search; 4 widths at `Kind::Tool`; both search states at `Kind::Tool`; both themes at `Kind::Tool`. That is 17 renders instead of 288 and every original assertion still runs on every value of every axis.

```rust
// now
for light in [false, true] { for width in [100, 149, 150, 200] {
    for kind in [User, Assistant, ..., QueuedSystem] { for search in [false, true] {
// change to
fn assert_selected_row_bold(light: bool, width: u16, kind: Kind, search: bool) { ... }
#[test] fn selected_row_bold_for_each_kind() { for kind in KINDS { assert_selected_row_bold(false, 150, kind, false) } }
#[test] fn selected_row_bold_at_each_width() { for width in [100, 149, 150, 200] { assert_selected_row_bold(false, width, Kind::Tool, false) } }
#[test] fn selected_row_bold_in_search_and_light() { for (light, search) in [(false, true), (true, false), (true, true)] { assert_selected_row_bold(light, 150, Kind::Tool, search) } }
```

### diff_backgrounds_preserve_text_positions

Every case renders a 250-row canvas twice (view and reference) at three widths and two themes, then compares cell by cell. The diff cases from `claude-tool-calls.jsonl` have far fewer lines than 250, so size the canvas to the tallest case: `EVENT_TOP + max visible lines + chrome`. Measured need: compute `layout.visible.len()` once per case at width 41 and use the maximum plus the panel chrome as the height.

```rust
// now
let area = Rect::new(0, 0, width, 250);
// change to
let area = Rect::new(0, 0, width, 19 + tallest_case_lines);   // smallest height that keeps every visible line of every case on screen
```

### task_event_search_loads_all_caches_matches_and_refreshes_growth

Three behaviours share one setup: search loads all matches across windows, a query edit reuses the cached transcript, and growth refreshes the match count. Split into three tests on the same helper. The search-loads-all test needs copies > one window (`refresh_event_for_list_rows(1)` reads one row, so 2 copies already cross the window) and as many `ScrollUp` as events to reach scroll 0; the cache and growth tests need only 1 copy.

```rust
// now
replace_transcript(&path, transcript_copies(&payload, 20, 0));
assert_eq!(app.view.matching_event_count(), 20);
for _ in 0..30 { scroll_event(&mut app, ScrollUp); }
// change to (search-loads-all test)
replace_transcript(&path, transcript_copies(&payload, 3, 0));
assert_eq!(app.view.matching_event_count(), 3);
for _ in 0..6 { scroll_event(&mut app, ScrollUp); }
// cache-reuse and growth tests start from transcript_copies(&payload, 1, 0)
```
