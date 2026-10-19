# Click waits and bounded selection

## Changes

- `src/app_ui_tests.rs`: both clicks use the existing `wait_for_click(&mut app)` helper, which waits through real `wait_for_input` calls. The first test asserts the click is no longer pending after settlement; the rendered branch editor and all subsequent assertions remain. All existing draw calls remain.
- `src/ui/tests.rs`: `returning_to_inactive_through_an_empty_tab_restores_its_selection` allows at most `view.rows.len()` Down events. Exhaustion panics with the event count, selected index and selected ID. The target and tab restoration assertions remain.
- No product code, mocks, installation or fabricated timestamps added. Other sessions' changes were left alone.

## Verification

Same commands before and after:

```sh
cargo test --bin agents app::ui_tests::
cargo test --bin agents ui::tests::
```

| Module | Before command wall time | After command wall time | Before test execution | After test execution |
|---|---:|---:|---:|---:|
| `app::ui_tests::` | 2.344s | 9.025s | 2.16s | 2.30s |
| `ui::tests::` | 2.120s | 4.936s | 1.98s | 1.80s |

Both runs: `app::ui_tests::` 32 passed, 0 failed; `ui::tests::` 461 passed, 0 failed, 1 existing ignored live-account test. The ignored test reads local credentials and live provider usage and requires the running Codex daemon; its annotation was unchanged.

Command wall time includes compilation and startup. Before compilation: 0.09s / 0.08s; after: 5.88s / 2.39s. Test execution times are Cargo's measured values; these measurements do not establish a speed improvement.

`git diff --check` passed. Targeted rustfmt check found a pre-existing formatting difference in a separate tuple near the end of `src/ui/tests.rs`; the same difference was reproduced from HEAD and left untouched.

Raw command outputs, command timings and the bounded runner: `click-loop-data/`. The runner uses a fixed 420-second deadline per phase and reports completion or failure once.
