# mobile tests slower than 1s

From one full `npm test` run on 2026-10-27, branch `fix-slow-tests`. Node tests are timed per test; shell and Python test scripts are timed per whole file.

| # | Seconds | Test | File |
|---:|---:|---|---|
| 1 | 6.27 | (whole file) | plugins/mobile/skills/check-mobile-design/tests/test_check_design.py |
| 2 | 5.63 | (whole file) | plugins/mobile/skills/ios-take-screenshot/tests/test_frame_diff.py |
| 3 | 5.48 | (whole file) | plugins/mobile/hooks/tests/test_phone_session_gate.py |
| 4 | 3.90 | (whole file) | plugins/mobile/skills/ios-take-screenshot/tests/test_stitch_screens.py |
