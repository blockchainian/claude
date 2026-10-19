# agents 并行测试调查

## 结果

- 配色：共享主题造成测试互相影响。干净 HEAD 副本里安排一次跨线程切换，原断言确定性复现 left: Some(Indexed(124))、right: Some(Indexed(29))；隔离测试主题后，同一交错通过。
- watcher：本机非沙箱、干净 HEAD 的 cargo test 和 nextest 各三轮全过，失败率分别 0/3、0/3。没有本机并发超时的根因证据。
- 原 tests-serial.log 实际仍有 watcher 超时；只有 tests-host-serial.log 全过。现有日志并非“并行失败、串行全过”。

## 1. manager_await_json_displays_multiline_strings_and_copies_drawn_text

### 复现

基线为 97f9faf7bc4473458e68e8843fa0790f32b6e440，通过 git archive 取出源码。工作树 tool_detail.rs 的三个未提交 hunk 都是格式调整，未涉及失败测试；初始 diff 保存在 color/initial.json，交付前重新比对。

| 对照 | 结果 | 日志 |
| --- | --- | --- |
| 原 HEAD、本机 cargo 全量三轮 | 原测试 3/3 通过 | watcher/cargo-test-{1,2,3}.log |
| 原 HEAD、本机 nextest 全量三轮 | 原测试 3/3 通过 | watcher/nextest-{1,2,3}.log |
| 工作树加屏障回归，尚未修复 | 失败；字符串颜色 124 对 29 | color/red.log |
| 干净 HEAD 只加同一个屏障回归 | 失败 | color/head-red-regression.log |
| 干净 HEAD 的原 manager 测试，加受控切换 | 原颜色断言失败，124 对 29 | color/head-red-manager.log |
| 最小修复，同一屏障回归 | 通过 | color/head-green-regression.log |
| 最小修复，同一原 manager 测试交错 | 通过 | color/head-green-manager.log |

受控交错只在独占副本中加入：绘制前设置浅色；生成参考样式前，让另一个真实线程调用深色 use_theme 并 join。原 fixture、绘制路径和断言不变；无 sleep、重试或 mock。完整探针 diff 为 color/manager-schedule.patch。交错通过后，副本的 tool_detail.rs 恢复到原 HEAD，再跑最终全量检查。没有改工作树的 tool_detail.rs。

### 根因证据

ui/highlight.rs 的 `THEME: Mutex<HighlightTheme>` 同时保存主题和 generation。Mutex 只保护一次高亮调用，不能保证测试先绘制、再查询参考颜色期间没有另一个测试切主题。浅色字符串为 Indexed(124)，深色为 Indexed(29)，与原失败及新受控复现一致。

已读到两个写者：ui::tests::task_log_selected_tool_highlighting_refreshes_when_theme_changes 直接切换深、浅、深；app::tests::terminal_theme_events_update_background_and_accents 经 App::apply_background 切换。写者拿了 theme::TEST_LOCK，manager 测试没有拿；这个锁没有形成共同隔离。SYNTAX_SET 是只初始化一次的不可变数据，不是这个颜色变化的来源。

共享主题可造成完全相同的失败；历史日志没有写者调度记录，不能指定当时是哪一个写者。原测试自然调度的六轮本机对照没有失败，和受控交错复现分别报告。

### 改动和提交

仅改 agents/src/ui/highlight.rs：测试构建的主题及 generation 改为线程局部状态；访问集中到 with_theme_state。非测试构建仍使用同一个全局 Mutex，保留加载、递增、加锁及 poison 处理行为。没有改产品配色或高亮算法。

新增 test_themes_do_not_cross_threads，使用两个屏障安排真实线程交错，验证另一个测试线程切主题不会改变本线程 JSON 样式。先运行失败，再做修复；HEAD 对照排除了其他会话未提交代码的影响。没有加锁串行化测试、分组、ignore 或改时限。

代码提交：2482af228745eb400406f09bb2f78b7274d313c4（test(agents): isolate syntax themes between test threads）。

## 2. appended_transcript_notifies_within_two_seconds

### 本机复现率

全部经权限升级，在非沙箱本机运行，基线源码为上述干净 HEAD。保留默认并发，未设置测试线程数、nextest 分组或重试。环境变量记录见 parallel-environment.json。

| 全量命令 | 轮次 | 通过 / 失败 | watcher | nextest watcher 用时 |
| --- | --- | --- | --- | --- |
| cargo test | 1 | 1533 / 0 | 通过 | — |
| cargo test | 2 | 1533 / 0 | 通过 | — |
| cargo test | 3 | 1533 / 0 | 通过 | — |
| cargo nextest run --no-fail-fast | 1 | 1533 / 0 | 通过 | 0.030 s |
| cargo nextest run --no-fail-fast | 2 | 1533 / 0 | 通过 | 0.019 s |
| cargo nextest run --no-fail-fast | 3 | 1533 / 0 | 通过 | 0.020 s |

每轮 cargo 有原有的 20 个单元测试、89 个集成测试忽略；nextest 有 109 个跳过。本次没有新增忽略。当前基线比用户旧日志多一个启用测试，旧日志仍按其原计数记录。完整日志、退出码和环境位于 watcher/，详见 watcher/report.md、watcher/runs.json、watcher/summary.json、watcher/environment.log。本机为 macOS 26.5.2、arm64、18 个逻辑 CPU，cargo/rustc 1.96.0-nightly、nextest 0.9.140。

### 环境证据与边界

新编译的同一个原生探针，在沙箱输出 FSEventStreamStart=0，在权限升级后的本机输出 FSEventStreamStart=1。源码 watcher/start.c、原始输出 watcher/start-sandbox.log / watcher/start-host.log 保留。

已核查 notify 7.0.0 的本地 src/fsevent.rs：它忽略 FSEventStreamStart 的 Boolean 返回值，之后才通过启动 channel 放行 watch()，所以原生启动失败仍可能得到注册成功，最终收事件超时。不能归因为“watch 返回时还没调用 Start”。探针匹配 notify 的启动 flags 和零 latency，但在线程、回调和目录上不同：直接证明的是探针启动被环境影响，Rust 历史超时的同类原因仍是推断；旧日志究竟运行在哪个环境未能确认。

测试有独立临时目录、channel、watcher，不修改环境或 cwd；后端有独立 OS 线程。六轮本机结果没有支持 FSEvents 延迟/合并、线程饥饿或同目录干扰。0/6 只是本次样本，没有证明长期失败概率为零。没有证据认定两秒时限不合理，保持不变。

改动：无。watcher 代码、测试、依赖均未改；提交 SHA：无。

## 3. 验证和所有权

修复后的独占副本为原 HEAD 加上述单文件修改；原 manager 测试没有受控切换。本节三轮全量及构建命令均在非沙箱本机运行；先前工作树 UI 定向检查在默认沙箱运行。

- cargo test 三轮：每轮 1534 通过、0 失败，原有单元测试忽略 20，集成测试忽略 89。
- cargo nextest run --no-fail-fast 三轮：每轮 1534 通过、109 跳过；watcher 分别 0.038 / 0.032 / 0.030 秒，两项目标测试均通过。
- cargo clippy --all-targets、cargo build --release：退出 0，有原有警告。
- 工作树 UI 定向并发检查：892 通过、0 失败、4 忽略，见 color/green-focused.log。
- 单文件 rustfmt --check 及 git diff --check：通过。

完整日志为 cargo-fixed-{1,2,3}.log、nextest-fixed-{1,2,3}.log、clippy.log、release.log；命令、退出码及解析后的计数为 validation.json 和 validation-summary.json。新增的一个通过测试是跨线程主题回归。固定副本源码校验和保存在 fixed-source-hashes.json。

共享分支期间合入了其他提交，因此另外清理独占 target 的 agents 包产物，对当前工作树再验证一次：cargo test 1537 通过、0 失败；nextest 全过；clippy 和 release 构建退出 0。见 current-*.log、current-validation.json 和 current-validation-summary.json。该轮包含其他会话的未提交改动；正式基线与修复对照仍使用固定 archive。当前源码与已验证副本的 highlight.rs 校验和一致，tool_detail.rs 未提交 diff 与初始记录一致。

首次 host 批次误用了共享 target，第三次 cargo 和三次 nextest 混入主线程尚未修复的新增回归。该批次全部排除出正式复现率，完整保留在 watcher/contaminated/。其中另一个颜色快照测试 claude_write_content_uses_path_syntax_and_preserves_unknown_text 也出现颜色差异（Dockerfile 的 Indexed(30) 对 Indexed(23)），没有修改该测试；干净 HEAD 六轮均通过。正式验证改用独立 COW target，强制从 archive 重编译，并保存实际二进制的 --list 确认未混入新增回归。

本次不安装，不修改时钟、时限或 watcher 测试，不覆盖或提交他人改动。临时调度探针只存在独占 archive 副本。最终只提交自己的源码和报告；推送目标 origin agents-panel。

## 4. 更正：使用现有 TEST_LOCK，保留正式主题存储

2482af22 的线程局部方案让测试与正式构建使用不同的主题存储，不符合要求。已通过 `git revert --no-edit 2482af22` 单独撤销，撤销提交为 08981ec4。该提交及其 test_themes_do_not_cross_threads 不再作为最终修复。以下结果替代前面关于线程局部修复的结论；前面的原始日志与 watcher 本机对照仍保留。

highlight.rs 与 2482af22 的父提交逐字一致：测试与正式构建继续共用全局 `Mutex<HighlightTheme>`，没有新增测试专用主题存储或主题机制测试。

### 最终改动

只在以下五个测试的函数入口加 `theme::TEST_LOCK`，guard 覆盖设置、绘制、参考颜色读取和所有断言。manager 和固定配色快照在持锁后显式设置默认深色主题。

| 文件 | 测试 | 需要隔离的共享状态 |
| --- | --- | --- |
| ui/tool_detail.rs | manager_await_json_displays_multiline_strings_and_copies_drawn_text | 绘制与 JSON 参考样式之间的全局语法主题 |
| ui/tool_detail.rs | json_sections_use_multiline_highlighting_and_placeholders_are_dim | 两次 Vec<Line> 比较中的样式 |
| ui/log_detail_regression_tests.rs | tool_detail_body_uses_default_foreground_and_preserves_syntax_highlighting | 先绘制，后生成 Rust/JSON 参考颜色 |
| ui/log_detail_regression_tests.rs | claude_write_content_uses_path_syntax_and_preserves_unknown_text | 固定快照中的语法配色 |
| ui/details_group_tests.rs | status_rule_keeps_github_pr_hyperlink_and_visible_click_target | 绘制与参考 thread_color 之间的全局 ACCENTS |

其余直接主题调用、颜色比较、快照及 helper 的审计见 lock-audit/report.md。已有 assert_script_styles 等 helper 自己持锁，不重复套锁；使用显式局部 Theme、固定语义色、仅检查文本/颜色存在性，以及现有线程局部 text accent 的比较没有加入这把共享锁。没有修改这些既有产品实现。

### 同一交错的红绿验证

独立 archive 基线为撤销后的 08981ec4，使用独占 COW target。临时探针只有测试调度代码，真实写者同样遵守现有 TEST_LOCK；没有改产品代码、颜色、时间或依赖，没有 sleep、重试或 mock。

- 读者绘制前设置浅色，生成参考颜色前通过 channel 放行写者。
- 写者先 try_lock 报告是否被挡住；拿不到锁就报告后正常阻塞 lock，最终切换深色。
- 未补锁：writer_blocked_before_reference=false，写者切深色后原断言失败，left Indexed(124)、right Indexed(29)。见 lock-fix/red.log。
- 补锁后：writer_blocked_before_reference=true，原测试全部断言通过；guard 释放后 writer_completed=true，并再次检查全局字符串颜色已变为深色 Indexed(29)。见 lock-fix/green.log。

这验证了现有全局存储的互斥：写者在整个读取和断言期间不能插入，读者结束后仍正常完成。受控探针只在独立副本；工作树仅新增五个 guard 和两处主题设置，共七行测试代码。调度脚本为 lock-fix/schedule.py，拥有的源码补丁为 lock-fix/owned.patch。

### 验证、提交和所有权

- 指定命令 `cargo nextest run -E 'test(/^ui::/)'`：891 通过、0 失败；754 跳过包含过滤掉的其他命名空间和原有忽略，并非新增 ignore。
- cargo test 默认多线程全量：1536 通过、0 失败；20 个单元测试、89 个集成测试为原有忽略。
- cargo clippy --all-targets、cargo build --release：退出 0，有既有警告；没有安装。
- 三个修改文件的 rustfmt --check、暂存补丁 diff --check：通过。

检查使用共享工作树和独占 target，先清理该 target 的 agents 包产物，避免副本与工作树复用错误的测试二进制。命令和退出码见 lock-fix/validation.json，完整日志为 lock-fix/nextest-ui.log、cargo-test.log、clippy.log、release.log；计数解析为 lock-fix/validation-summary.json。

最终修复提交：b5b05a3f72e81c25bb064944798434bd9d98ee63（test(agents): guard theme-dependent assertions with shared lock）。通过 git apply --cached 仅暂存 lock-fix/owned.patch 中自己的 hunk；提交为三个测试文件、七行新增。去掉这七行后，三个文件与编辑前保存的工作树内容逐字相同，tool_detail.rs 中其他会话的格式调整完整保留在未暂存区；校验结果为 lock-fix/ownership.json。没有提交其他会话的 hunk。
