# intel tests slower than 1s

From one full `npm test` run on 2026-10-27, branch `fix-slow-tests`. Node tests are timed per test; shell and Python test scripts are timed per whole file.

## transcribe

| # | Seconds | Test | File |
|---:|---:|---|---|
| 1 | 31.06 | live segments a generated clip and prints a JSON summary | plugins/intel/skills/transcribe/tests/transcribe.test.mjs |
| 2 | 22.65 | batch transcribes a generated clip and prints JSON | plugins/intel/skills/transcribe/tests/transcribe.test.mjs |

## translate

| # | Seconds | Test | File |
|---:|---:|---|---|
| 1 | 25.29 |  | plugins/intel/skills/translate/epub/tests/test_render.py |

## digest

| # | Seconds | Test | File |
|---:|---:|---|---|
| 1 | 10.78 |  | plugins/intel/skills/digest/tests/test_pdf_highlights.py |
| 2 | 2.35 |  | plugins/intel/skills/digest/tests/test_fetch_source.py |

## case-study

| # | Seconds | Test | File |
|---:|---:|---|---|
| 1 | 6.86 | an Exa answer that is an error is no result: a 5xx or a timeout is asked again, any other error gives nothing | plugins/intel/skills/case-study/tests/gate.test.mjs |
| 2 | 1.86 | exa retries on 429, stops asking for ten minutes after a 402, and otherwise returns the answer | plugins/intel/skills/case-study/tests/gate.test.mjs |
