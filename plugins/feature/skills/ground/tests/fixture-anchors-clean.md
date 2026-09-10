# Fixture: every anchor's sentence names a symbol found near the cited lines
Base: `321ab7087` on branch `browse`, 2026-09-09.

- `launchpadLabel()` returns the `LAUNCHPAD_LABELS` entry, else the name unchanged when it has a space, else the first character uppercased — `website/src/components/chadwallet/token/launchpad.ts:26-36`; the `LAUNCHPAD_LABELS` lookup is at `:27`; `launchpadLabel` runs from `:26-…`.
- `'/v1/trenches'`, `/defi/token_security` and `proxy/src` are routes and a directory, not files; `launchpadLabel` is still at `:26`.
- `mapGmgnNewTokenRow` calls `mapGmgnTrenchesRows([row], chain)[0]` at `proxy/src/tokenListSources.ts:841`.
- The constants list "Constants:" names `GRADUATED_FLOORS` — `docs/token-lists/graduated-and-bonding-lists.md:77-78`.
- `TokenInfo.launchpad` fields — `mobile/shared/data/api/client.ts:211-214`.

```
proxy/src/no-such-file.ts:12 inside a fenced block is output, not an anchor
```
