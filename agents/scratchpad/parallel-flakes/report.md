# agents 并行测试调查

## 结论

- 文件锁：测试错误地把父进程关闭文件等同于最后一个持锁描述符关闭。真实 fork/pipe 探针确定性复现这个错误；只改测试，显式解锁。代码提交 `0f589ba7`。
- 文件通知：本次宿主环境的单独、默认并行 cargo、全量并行 nextest 均通过，原来的并行专属失败未复现。默认沙箱下单独和两种全量均超时；宿主对照通过，沙箱再次单独运行仍超时。不能把这个结果归为并行饥饿，也没有证据认为两秒太短。watcher 源码和测试不改。
- 没有安装，没有改时限、加 sleep/重试、串行化、加分组、ignore、mock 或改时间。没有改另一会话的文件。

## 1. providers::codex::tests::locks_require_a_holder_not_just_a_file

### 复现结果

| 运行方式 | 修改前结果 | 证据 |
| --- | --- | --- |
| 单独 cargo test，默认沙箱 | 通过 | `lock-alone.log` |
| cargo test 默认多线程，默认沙箱 | 锁测试通过；通知测试失败 | `cargo-baseline.log` |
| cargo nextest run --no-fail-fast，全量默认并行，默认沙箱 | 锁测试通过；通知测试失败 | `nextest-baseline.log` |
| 历史全量日志 | 锁测试在 drop 后的断言失败 | `../rate-limit-optionals/test.log`，原测试第393行 |
| 历史单独日志 | 通过 | `../rate-limit-optionals/lock-isolated.log` |

本次原始全量没有再现锁测试失败；下面的真实系统调用探针复现了导致该断言失效的生命周期。没有捕获历史失败的子进程 PID，因此不声称确认了当时具体是哪一个测试创建了该子进程。

### 根因证据

`lock_held` 打开文件并尝试共享锁；已有独占持锁者时正确返回 true。每个测试目录已经独立，不需要文件名冲突或环境变量竞争。

Rust 的 File 锁由底层 flock 管理。fork 会继承描述符；FD_CLOEXEC 只在 exec 时关闭继承描述符，不能消除 fork 到 exec 之间的窗口。项目 `src/pty.rs::Pty::spawn` 使用 pre_exec，Rust 的该启动路径会 fork，所以别的并行测试确实能处于这个窗口。父进程 drop(writer) 后，只要子进程还有继承的描述符，锁就仍然存在。

`lock/inheritance.rs` 用真实 File 锁、fork 和 pipe 屏障，不用 sleep、重试或时间假设。子进程等待父进程测量，父进程完成后放行并 waitpid 回收。原始输出 `lock/inheritance.log`：

```
explicit_unlock=false, held_after_parent_drop=true, held_after_child_exit=false
explicit_unlock=true, held_after_parent_drop=false, held_after_child_exit=false
```

第一行证明 drop 不保证解锁，第二行证明显式 unlock 即使子进程仍持描述符也会释放锁。产品探测没有误报；错误在测试的“这是最后一个描述符”假设。

### 改动

仅 `agents/src/providers/codex/tests.rs`：

```diff
-    drop(writer);
+    // Parallel process launches can inherit this descriptor until exec finishes.
+    writer.unlock().unwrap();
```

仍验证不存在路径、存在但未锁定、独占锁持有、显式释放四个状态；最后的检查发生时文件仍然打开。产品实现未改。

代码提交：`0f589ba7`（test(agents): release lock explicitly before checking holders）。

### 验证

- 真实 fork/pipe 两种生命周期验证通过。
- 修改后的单独测试通过：`lock/fixed-isolated.log`。
- 修改后的宿主 cargo 全量和 nextest 全量通过，详见第3节。

## 2. sessions::watcher::tests::appended_transcript_notifies_within_two_seconds

### 复现结果

| 环境 / 运行方式 | 结果 | 证据 |
| --- | --- | --- |
| 默认沙箱 / 单独 cargo | 两秒内没有通知，失败 | `watcher-alone.log` |
| 默认沙箱 / cargo test 默认多线程 | 1527通过、1失败、20忽略；只失败通知测试 | `cargo-baseline.log` |
| 默认沙箱 / nextest 全量默认并行 | 1527通过、1失败、109跳过；只失败通知测试 | `nextest-baseline.log` |
| 宿主 / 单独 cargo | 通过 | `watcher/host-alone.log` |
| 回到默认沙箱 / 单独 cargo | 仍超时 | `watcher/sandbox-alone-confirm.log` |
| 宿主 / cargo test 默认多线程 | 全部启用测试通过 | `cargo-host-baseline.log` |
| 宿主 / nextest 全量默认并行 | 全部启用测试通过；通知测试0.023秒 | `nextest-host-baseline.log` |

宿主两次全量是在文件锁测试修改后运行；watcher 文件在所有对照中相同。原先“单独通过而并行失败”的宿主现象未复现。

### 根因证据及边界

当前可复现的问题跟沙箱能力相关，不以并行为必要条件。原生启动探针 `watcher/start.c` 在相同二进制下输出：

```
# 默认沙箱，watcher/start-sandbox.log
FSEventStreamStart=0
# 宿主，watcher/start-host.log
FSEventStreamStart=1
```

notify 7.0.0 的本地源码 `src/fsevent.rs` 丢弃 FSEventStreamStart 的 Boolean 返回值，随后把 runloop 通过 channel 返回给调用方，因此启动失败也可能让 watch() 返回 Ok。该依赖行为说明为何测试无法在注册阶段看见错误，而是在等待通知时超时。探针和 notify 使用相同的 SinceNow、0.0 latency、FileEvents|NoDefer 和默认 runloop mode；探针使用主线程、空回调和 /private/tmp 目录，notify 使用专用线程和测试 TMPDIR。因此直接证实的是原生探针在沙箱启动失败；Rust 测试中的同类启动失败是受到源码和环境对照强支持的推断，没有声称直接截获了其内部返回值，也没有查明具体是哪条沙箱规则拒绝了启动。

排查不支持以下说法：

- 共享测试文件路径：测试使用独立 tempfile 目录和 channel。
- 全局环境变量或当前工作目录污染：该测试不修改它们。
- 公用线程池饥饿：notify 的这个后端有自己的线程；单独跑也失败。
- 简单的“watch 返回时还没调用 Start”：notify 在 Start 调用之后才经 channel 完成注册返回。不能据此证明后端所有异步行为都没有问题。

没有事件证据把 notify teardown 的设备事件清理操作与历史失败关联起来，不将其当作根因。

### 两秒时限

测试计时的是 watch 注册返回之后，一次真实 append 的通知送达；它不是 dashboard 刷新的完整端到端承诺。`src/refresh.rs` 使用50毫秒 debounce、启用 watcher 时5秒周期刷新、注册报错后1秒轮询。这些实现没有给出原生通知必须两秒送达的严格产品承诺；但本次宿主通知很快，也没有“两秒阈值不合理”的证据。保持该断言不变。

### 改动、提交、验证

watcher 产品代码、测试和依赖均未改；代码提交 SHA：无。没有通过跳过测试或改断言掩盖沙箱失败。宿主单独和两种全量已通过；当前沙箱的原生能力限制依然存在。notify 吞掉启动失败的行为没有在本次引入依赖补丁；原并行失败的具体根因仍不能确认。

## 3. 最终检查与交付

命令均在 agents/ 运行；全量使用默认并发设置，没有设置 RUST_TEST_THREADS、nextest 分组或重试。

| 检查 | 结果 | 日志 |
| --- | --- | --- |
| 宿主 cargo test | 1528通过，0失败，单元测试20忽略；另89个原有集成测试忽略 | `cargo-host-baseline.log` |
| 宿主 cargo nextest run --no-fail-fast | 1528通过，0失败，109跳过 | `nextest-host-baseline.log` |
| cargo clippy --all-targets | 通过，有原有警告 | `clippy.log` |
| cargo build --release | 通过 | `release.log` |
| git diff --check，限定本次测试文件 | 通过 | 命令退出0 |

修改时间早于构建后的测试二进制，宿主全量验证覆盖了显式 unlock 版本。`baseline.json`、`host-baseline.json` 和 `gates.json` 保存命令、退出码与耗时。

额外发现：对该测试文件运行 rustfmt --check 失败，差异都在本次改动之外；取 HEAD 原文运行同样失败，证据 `rustfmt-head.log`。未扩大格式化范围，未修改那些代码。

代码提交和报告提交均限定自己的路径；推送目标为 origin agents-panel。推送结果见报告提交之后的会话结果。不安装。
