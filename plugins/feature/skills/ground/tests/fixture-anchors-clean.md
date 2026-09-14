# Fixture: every anchor's sentence names a symbol found near the cited lines
Base: `@SHA@` on branch `main`, 2026-09-14.

- `parseConfig()` returns the `CONFIG_MAP` entry, else the key unchanged when it has a slash, else the value uppercased — `src/app.ts:26-36`; the `CONFIG_MAP` lookup is at `:27`; `parseConfig` runs from `:26-…`.
- `'/v1/status'`, `/defi/health` and `src/lib` are routes and a directory, not files; `parseConfig` is still at `:26`.
- `mapRows` calls `normalizeRow(row)` at `src/rows.ts:12`.
- The constants list "Constants:" names `FLOOR_LIMITS` — `docs/constants.md:6-7`.
- `UserRecord.email` field — `src/apiClient.ts:9-12`.

```
src/no-such-file.ts:12 inside a fenced block is output, not an anchor
```
