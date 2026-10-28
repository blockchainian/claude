## Criteria

Every test should finish in under 1s.

Never:

1. Change product code. If the cause of slowness is in product code, name that code instead of changing it.
2. Cut coverage. Every product behavior a test checks must stay checked.
3. Fake the flow. Don't age timestamps, use fake clocks, skip frames the real app draws, or move the test away from the real flow.

Allowed fixes:

1. Split unrelated behaviors. A test that checks several unrelated product behaviors becomes one test per behavior, and every original assertion survives in exactly one of them. Behaviors of different panels are unrelated, and so are different triggers of the same effect, such as mouse scrolling and keyboard scrolling.
2. Cut data and fixtures. Shrink data or fixtures the behavior doesn't need to the minimum that still tests it. Where the test itself replicates data or repeats operations, cut the copies and the repetitions.
3. Break up combinatorial tests. Replace a test that runs every combination of several code paths with one unit test per path.
4. Remove unneeded waits. Shorten fixed sleeps, polling intervals and waits longer than the event they wait for, or wait for the event itself.

Verdicts:

| Fixable | Meaning |
|---|---|
| fixable | The allowed fixes bring the test, and every test split from it, under 1s. |
| partly fixable | The allowed fixes make it faster, but at least one test stays over 1s. |
| blank | None of the allowed fixes helps. |
