# AGENTS.md

> Before working in a gstack source worktree, read its `AGENTS.md`.

This skill is personal patches and build tooling for gstack's `browse` CLI.

## Modules

- patches/ -- ordered personal patches
- scripts/ -- develop, check, and build tools
- gstack/ -- the `https://github.com/garrytan/gstack.git` submodule (MIT); its committed pointer is the build baseline
- GSTACK_COMMIT -- the same pointer as a file, for installs that carry no submodule
- .worktrees/ -- disposable worktrees: `develop`, `check-patches`

Keep only personal patches, workflow scripts, and their documentation here. Never import upstream source or history.

## Patches

Apply patches in filename order.

Export a new patch relative to the committed patched development baseline, never relative to the unpatched gstack tree.

Keep each patch minimal: only the code its behaviour needs, with one focused test per behaviour. Name a patch by its user-visible effect, not its mechanism. Do not put generated version or lockfile changes into a feature patch; browse builds with upstream's `package.json` and `bun.lock`.

To change an existing patch, recreate the baseline before it and amend that patch. Never add a patch that undoes another.

| Patch | What it changes |
|---|---|
| `0001-restart-daemon-on-new-build.patch` | The CLI passes its own version to the daemon it starts (`BROWSE_BINARY_VERSION`), which records it as `binaryVersion`; a CLI from a different build then restarts the daemon on its next command |
| `0002-open-headed-window-on-chosen-display.patch` | The headed window opens on the display `BROWSER_DISPLAY` names (environment, else `~/.config/web/.env`), through `--window-position` from `scripts/displayOrigin.swift`, which `build.sh` copies into `browse/scripts/`; adds `browse/test/browser-display.test.ts` |
| `0003-open-headed-tabs-without-stealing-focus.patch` | Headed `newTab` opens its page with CDP `Target.createTarget({ background: true })` instead of `context.newPage()`, so Chromium does not take macOS focus; adds `browse/test/background-tab.test.ts`, whose headed part runs only with `BROWSE_HEADED_TESTS=1` |
| `0004-show-only-the-pinned-tab-logs.patch` | Commands pinned to a tab (`BROWSE_TAB` / `--tab-id`) read and clear only that tab's console and network entries, and a response's status goes to its own tab's request; adds `browse/test/tab-scoped-logs.test.ts` |
| `0005-stop-duplicate-tabs-and-logs-in-headed-mode.patch` | Headed `newTab` adopts the tab id the context's `page` handler already gave its page, so each new tab appears once in `tabs` and logs each request once; adds `browse/test/headed-newtab.test.ts`, which runs only with `BROWSE_HEADED_TESTS=1` |

The build checks out only `browse/`, `lib/`, `patches/` and the root files, so these upstream parts are absent from an install:

| Upstream | Effect |
|---|---|
| `extension/` | headed mode starts without the gstack sidebar extension |
| `hosts/` | `browse pair-agent --local <host>` cannot write its host config |
| `browser-skills/` | no bundled browser skills; `~/.gstack/browser-skills/` still loads |

## Tools

Run these from this skill's directory.

### Set up gstack

- `git submodule update --init gstack` — fetches the submodule in an existing clone of the plugin repository

### Upgrade gstack

- `git -C gstack fetch origin main` — fetches the latest gstack commits
- `git -C gstack checkout --detach origin/main` — moves the submodule to them
- `./scripts/check-patches.sh` — checks the patches against the new baseline; a conflict leaves `.worktrees/check-patches` for inspection
- `git -C gstack rev-parse HEAD > GSTACK_COMMIT` — records the new pointer for installs
- `git add gstack GSTACK_COMMIT patches` — stages the pointer and the repaired patches for one commit
- bump the plugin version, then `npm run test:web` from the repository root

> **Warning**
>
> - Never run `git submodule update --remote`; builds must use the committed pointer
> - To abandon an upgrade, preserve any changes, then run `git submodule update gstack`

### Develop a patch

- `./scripts/develop.sh` — creates the development worktree in `.worktrees/develop`, with all patches committed as the baseline
- `bun install --frozen-lockfile` — installs upstream's dependencies in the worktree
- `cp ../../scripts/displayOrigin.swift browse/scripts/` — adds the display lookup the tests need; leave it unstaged, `build.sh` adds it at build time
- `git add <feature-files>` — stages the feature in the worktree root, including new files and tests
- `git diff --binary --cached HEAD > ../../patches/<NNNN>-<name>.patch` — exports the staged feature as a patch
- `./scripts/check-patches.sh` — checks that all patches apply in order

> **Warning**
>
> - Keep `gstack/` clean; develop only in the worktree created by `scripts/develop.sh`
> - Export before committing in the worktree, so `HEAD` still names the patched baseline
> - If other patches changed while you were developing, test the complete stack in a fresh worktree
> - Never launch a headed browser without the user's go-ahead: headed tests open visible windows and run only with `BROWSE_HEADED_TESTS=1`

### Build

- `./scripts/build.sh [--dest <dir>]` — fetches gstack at `GSTACK_COMMIT`, applies the patches, installs and compiles; default dest `${CLAUDE_PLUGIN_DATA:-~/.claude/plugins/data/web-blockchainian}/browse/<build id>`
- `../../bin/browse` — the launcher runs `build.sh` on first use of each build id (`scripts/build-id.sh`: a digest of `GSTACK_COMMIT`, the patches and the build scripts)

A new build id means a new binary, and each running daemon restarts on its next command, losing its tabs.

## Checks

- **patches**: `./scripts/check-patches.sh`
- **browse**: `npm run test:web` from the repository root, with `BROWSE_HEADED_TESTS` unset

## Version control

- Commit each coherent verified change with a plugin version bump, push it, and update the installed plugin.
- Commit `gstack` pointer changes together with `GSTACK_COMMIT` when upgrading upstream.
- Never push personal development commits to the gstack remote.
