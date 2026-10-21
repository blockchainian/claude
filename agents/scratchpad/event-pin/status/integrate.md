# 整合

## 已完成

- 修正旧 UI 测试：日志列表的 `p` 现在必须发出完整置顶操作；详情和提示输入仍不能误触任务快捷键。完整核对任务、来源、记录位置，以及光标所在的日志身份。
- 修正新增 App 测试的 cloned_ref_to_slice_refs 警告。
- 补齐真实 fixture 测试：取消后重新置顶回顶部；运行中 Claude 工具写出结果后，时间线更新、置顶快照不变，重新置顶才包含结果。
- 提交：4e50fd38、ab23809d。未 push，未安装。
- fd00a628 的 watcher 实现和测试原样单独编译，沙箱中同样出现 2 秒通知超时；两个文件与当前版本逐字节相同，未修改。
- fd00a628 完整 clippy 已运行；新增警告已消除，其余警告均有起点对应位置。比较记录：integrate-warning-comparison.json。

## 设计测试覆盖

| 条目 | 覆盖位置 |
|---|---|
| 切换置顶、取消、重新置顶顺序、多条先后 | task_event_list_tests 的 toggle、order、repin 测试 |
| 时间线保留、从时间线取消、耗时排序顺序 | task_event_list_tests、event_search::pinned_render_tests |
| store 重载后保持顺序 | state::tests::task_pinned_events_round_trip_order_and_replace |
| Claude/Codex 多记录工具按偏移重读一致 | 两家 pinned_reader_tests::pinned_records_restore_real_events_before_refresh |
| 来源不匹配、偏移越界清理 | app::event_pin_tests::first_load_recovers_pins_and_removes_invalid_records_and_sources |
| inactive 清整条任务状态和任务置顶 | app::event_pin_tests 的 inactive_snapshot 两项测试 |
| Codex 合并消息、Claude 工具来源一致 | 两家 pinned_reader_tests 完整事件相等断言 |
| 排队提示词、排队通知、空来源拒绝 | task_event_list_tests::event_pin_rejects_missing_identity_queued_and_inactive |
| 运行中工具快照与重新置顶 | task_event_list_tests::event_pin_running_tool_keeps_snapshot_until_repinned |
| 重复 inactive 刷新不再写 store | app::event_pin_tests::inactive_snapshot_forgets_all_task_state_once |
| 清掉停放置顶 | task_event_list_tests::event_pin_snapshot_survives_update_and_clear_parked |
| 切走切回保留置顶光标 | task_event_list_tests::event_pin_cursor_survives_task_switch |
| 置顶独立搜索过滤 | event_search::pinned_tests::pinned_search_uses_snapshot_body_without_timeline_indices |
| 一行重叠两条置顶时全部取消 | task_event_list_tests::event_pin_order_and_overlapping_records |
| Codex 单组坏记录不影响其他组 | pinned_reader_tests::pinned_malformed_completion_only_discards_its_group |

## 门禁

- 初次沙箱全量：1939 passed、2 failed、150 skipped。失败为旧快捷键断言和起点同样失败的 watcher。
- 修正后定向：7 passed、0 failed；新增覆盖定向：2 passed、0 failed。
- 最终宿主 `cargo nextest run --no-fail-fast`：1943 passed、0 failed、150 skipped，退出 0。
- 最终 `cargo clippy --all-targets`：退出 0；和 fd00a628 对照无新增警告，14 处既有警告保留。
- 最终 `cargo build --release`：退出 0。
- 最终日志：integrate-final-nextest.log、integrate-final-clippy.log、integrate-final-release.log。
- 初始默认 CARGO_TARGET_DIR 指向其他目录，锁文件访问失败；所有实际构建均显式使用本工作树 agents/target。
- 没有读其他工作树，所有基线源码均来自本工作树的 git 对象。

## 未完成及剩余失败

- 无。宿主全量包含 watcher 并通过；沙箱中的基线通知失败已记录，未改代码。
- 本次仅改测试，未改产品实现或界面文案；没有安装、push 或改其他目录。
