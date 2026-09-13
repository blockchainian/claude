# Plan: sample

## Workstreams

### `trades` — return sorted trades

1. sort by timestamp desc — `sortTrades()` `proxy/src/trades.ts:20`.

Tests: trades sorted desc `[AC1]`; empty wallet yields `[]` `[AC2]`; pagination `[AC3]`.
Files: `proxy/src/trades.ts`.
