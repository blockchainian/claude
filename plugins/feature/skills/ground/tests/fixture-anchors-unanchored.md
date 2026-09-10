# Fixture: a function's behaviour is claimed without a line anchor
Base: `321ab7087` on branch `browse`, 2026-09-09.

- Mapping lives in `website/src/components/chadwallet/token/launchpad.ts`: `launchpadLabel()` title-cases the raw name.
- `launchpadLabel` is tested through `TokenHeader` — `website/src/components/chadwallet/token/launchpad.ts:26-36`.
