# mobile tests slower than 1s

From one full `npm test` run on 2026-10-27, branch `fix-slow-tests`. Node tests are timed per test; shell and Python test scripts are timed per whole file.

## check-mobile-design

| # | Seconds | Test | File |
|---:|---:|---|---|
| 1 | 6.27 |  | plugins/mobile/skills/check-mobile-design/tests/test_check_design.py |

## ios-take-screenshot

| # | Seconds | Test | File |
|---:|---:|---|---|
| 1 | 5.63 |  | plugins/mobile/skills/ios-take-screenshot/tests/test_frame_diff.py |
| 2 | 3.90 |  | plugins/mobile/skills/ios-take-screenshot/tests/test_stitch_screens.py |

## hooks

| # | Seconds | Test | File |
|---:|---:|---|---|
| 1 | 5.48 |  | plugins/mobile/hooks/tests/test_phone_session_gate.py |
