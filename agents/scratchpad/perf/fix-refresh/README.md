# Refresh probe

正式对照必须从同一个 `git archive HEAD` 副本分别构建修前、修后二进制，并记录应用补丁。不要用现有 `before-agents` 推断源 SHA；它只用于脚本短测。

从对应 archive 副本的 agents/ 工作目录启动，在宿主权限运行，真实 HOME 不加 provider 目录覆盖；probe.py 和输出目录可传绝对路径，metadata 记录 measurement_cwd。每次监控固定 360 秒截止，输出日志和一个 completion/failure/timeout 信号。每个子进程另有 120 秒截止。每个模式保存独立摘要，失败保留已完成项、ANSI、可读屏幕和 wait status，不改写其它模式的数据。

```sh
UV_CACHE_DIR=/private/tmp/agents-fix-refresh-uv python3 agents/scratchpad/perf/fix-refresh/run.py before-startup uv run --with pyte python agents/scratchpad/perf/fix-refresh/probe.py <before-binary> agents/scratchpad/perf/fix-refresh before --source-head <40-char-SHA> --mode startup
UV_CACHE_DIR=/private/tmp/agents-fix-refresh-uv python3 agents/scratchpad/perf/fix-refresh/run.py before-interaction uv run --with pyte python agents/scratchpad/perf/fix-refresh/probe.py <before-binary> agents/scratchpad/perf/fix-refresh before --source-head <40-char-SHA> --mode interaction
UV_CACHE_DIR=/private/tmp/agents-fix-refresh-uv python3 agents/scratchpad/perf/fix-refresh/run.py before-exit uv run --with pyte python agents/scratchpad/perf/fix-refresh/probe.py <before-binary> agents/scratchpad/perf/fix-refresh before --source-head <40-char-SHA> --mode exit --sample
```

把 before 换成 after 并换二进制重跑。默认首帧 3 次、交互 100 次、启动期间退出 3 次（启动后 400 ms 发送 Ctrl+C）。`--mode all` 可以连跑；`--actions`、`--startup-runs`、`--exit-runs` 可用于短测。`--ready-wait` 控制交互前首帧后的等待，默认 3 秒。交互无真实选中任务或出现沙箱错误会失败。

```sh
python3 agents/scratchpad/perf/fix-refresh/report.py before after
```

报告脚本要求三个模式全部成功且二进制、显式源 SHA 一致；修前、修后 SHA 必须相同。数据不全时拒绝覆盖 report.md。终端时延是字节响应，可能混入独立刷新输出；不宣称精确识别输入对应帧。退出重叠 sample 若不存在或没有刷新栈，只能说未证实刷新正在运行。

没有创建 tmux，也不需要清理 tmux server。若另加 tmux 探针，结束时执行 `tmux -L <name> kill-server`。不要提交 before-agents 或其它二进制。

交互点击按当前选中任务 cursor.y 和可见 ●/○ 行选择不同叶子任务，排除有下级的只读父级标题；每次记录 click_x/click_y、selected_y_before、target_row 和 selected_y_after。固定点击坐标的旧诊断不得混入正式交互对照。
