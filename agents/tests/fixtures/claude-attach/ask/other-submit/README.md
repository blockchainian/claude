# Other submission capture

Claude Code 2.1.302; Haiku 5.5, low effort; disposable background session in a temporary directory. `two-initial.raw` is verbatim output from a real 200-column by 50-row `claude attach` in tmux, captured before answering either question. Both questions have the two options Red and Green. The recording includes Claude synchronized-output boundaries around the question screen.

The regression tests consume the original bytes across read boundaries, including a split before the final synchronized-output marker. They exercise the production screen parser and question readiness checks.
