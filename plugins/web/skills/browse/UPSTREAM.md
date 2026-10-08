# Upstream

`vendor/gstack/` is [garrytan/gstack](https://github.com/garrytan/gstack) (MIT,
see `vendor/gstack/LICENSE`) at commit
`0d1bd561` (v1.79.0.0),
cut down to the files the `browse` CLI needs. Vendored files are byte-identical
to upstream; this skill's own changes live in `patches/` and are applied at build
time by `scripts/build.sh`, which the plugin's `bin/browse` launcher runs on first
use of each build id (`scripts/build-id.sh`).

## What is vendored

The list lives in `scripts/vendor.sh`:

| Path | Why |
|---|---|
| `browse/src/` | the CLI and the daemon |
| `browse/bin/`, `browse/scripts/` | upstream helpers; `build-node-server.sh` is run by the build |
| `browse/test/test-server.ts`, `browse/test/fixtures/basic.html` | the harness the patches' own tests need |
| `lib/egress-receipt.ts`, `lib/fs-atomic.ts` | imported by `browse/src` |
| `patches/playwright-core@1.62.1.patch` | upstream's own Playwright patch, applied by `bun install` |
| `LICENSE` | MIT |

## Not byte-identical

- `package.json`: only what browse needs — `playwright`, `diff`, `socks`,
  `sharp` (full-page screenshot downscaling) and `cross-spawn` (the Node server
  bundle's polyfill), pinned to the versions upstream's `bun.lock` resolved;
  upstream's `ip-address` override and Playwright `patchedDependencies` kept.
- `bun.lock`: regenerated for that `package.json`; every package resolves to the
  same version as in upstream's lock.

## Left out

| Upstream | Effect of leaving it out |
|---|---|
| `extension/` | headed mode starts without the gstack sidebar extension |
| `hosts/` | `browse pair-agent --local <host>` cannot write its host config |
| `browser-skills/` | no bundled browser skills; `~/.gstack/browser-skills/` still loads |
| `@ngrok/ngrok` | `pair-agent` tunnels fail to start |
| `@huggingface/transformers` | the ML prompt-injection classifier sidecar is unavailable |
| every other gstack skill, `design/`, `make-pdf/` | not used |

## Patches

| Patch | What it changes |
|---|---|
| `0001-closing-the-last-tab-no-longer-wedges-the-daemon.patch` | After the last tab closes, `restart`, `stop`, `newtab` and `goto` still work instead of failing with "No active page"; a local fix from our gstack clone, absent from upstream at `0d1bd561`; adds `browse/test/no-tab-recovery.test.ts` |
| `0002-scope-console-and-network-logs-to-pinned-tab.patch` | Commands pinned to a tab (`BROWSE_TAB` / `--tab-id`) read and clear only that tab's console and network entries; adds `browse/test/tab-scoped-logs.test.ts` |

## Upgrading

1. `bash scripts/vendor.sh <gstack-checkout> <commit>` — re-vendors the paths
   above, keeps this `package.json` and `bun.lock`, and fails if a patch no
   longer applies.
2. Compare `package.json` with upstream's at the new commit, then regenerate
   `bun.lock` with `bun install` in a scratch copy and check its versions match
   upstream's lock.
3. Update the commit above, run `npm run test:web`, and bump the plugin version.
   The build id changes with the vendor tree, so the launcher rebuilds on next use.
