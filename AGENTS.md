# AGENTS.md

> Before working in a component, read its `AGENTS.md` and `README.md`.  
> Never hard-code a path of this machine: the repo is public.  
> Never import a plugin script by its versioned cache path.

This repo is the `blockchainian` plugin marketplace for Claude Code and Codex, and the only home of all its plugins.

## Modules

- .claude-plugin/ -- the Claude Code catalog
- .agents/plugins/ -- the Codex catalog
- plugins/ -- one directory per plugin
- agents/ -- `agents`, the Rust session dashboard
- git/ -- `git`, the VS Code extension that opens a commit, a repository folder or a pull request from a `vscode://blockchainian.git/` link
- tests/ -- the marketplace suite

## Development

- Name tools by what they do.
- A tool offers the capability up to its hard limit; callers decide budgets.
- A tool supports only its current data format; no compat code.

## Checks

The per-plugin gate a plan's `Checks` command composes:

- **validate**: `npm run validate`
- **tests**: the plugin's command under Tests

## Tests

> Run the relevant tests.

- `node --test tests/marketplace.test.mjs` — the marketplace manifest and shared-engine checks
- `node --test plugins/codex/tests/*.mjs` — codex-manager against a fake app-server daemon
- `node --test plugins/proxy/skills/inspect-app-traffic/tests/*.mjs` — the capture logic and WireGuard key derivation
- `npm ci --prefix plugins/intel/skills/fetch-x-mentions/scripts && npm ci --prefix plugins/intel/skills/fetch-app-reviews/scripts && npm ci --prefix plugins/intel/skills/unminify/scripts && node --test plugins/intel/skills/transcribe/tests/*.mjs && uv run plugins/intel/skills/digest/tests/test_fetch_source.py && node --test plugins/intel/skills/digest/tests/*.mjs && uv run plugins/intel/skills/digest/tests/test_pdf_highlights.py && node --test plugins/intel/skills/translate/epub/tests/*.mjs && uv run plugins/intel/skills/translate/epub/tests/test_render.py && node --test plugins/intel/skills/find-domain-names/tests/check.test.mjs && node --test plugins/intel/skills/download-book/tests/*.mjs && node --test plugins/intel/skills/case-study/tests/*.mjs && node --test plugins/intel/skills/analyze-appstore-reviews/tests/*.mjs && node --test plugins/intel/skills/fetch-x-mentions/tests/*.mjs plugins/intel/skills/fetch-x-posts/tests/*.mjs plugins/intel/skills/fetch-x-user-posts/tests/*.mjs plugins/intel/skills/analyze-x-mentions/tests/*.mjs plugins/intel/skills/analyze-x-user/tests/*.mjs plugins/intel/skills/analyze-x-users/tests/*.mjs plugins/intel/skills/fetch-tiktok-mentions/tests/*.mjs plugins/intel/skills/fetch-x-feed/tests/*.test.mjs plugins/intel/skills/manage-x-account/tests/*.test.mjs && node --test plugins/intel/skills/unminify/tests/*.test.mjs` — the intel skills; the render suites need Chrome and uv
- `node --test plugins/web/skills/heap-snapshot-leaks/tests/*.mjs plugins/web/skills/browse/tests/*.mjs && uv run plugins/web/skills/check-web-design/tests/test_check_design.py` — the heap-snapshot diff and the design check; needs uv
- `node --test plugins/mobile/tests/*.mjs && uv run plugins/mobile/hooks/tests/test_phone_session_gate.py && node --test plugins/mobile/skills/ios-take-screenshot/tests/*.mjs && uv run plugins/mobile/skills/ios-take-screenshot/tests/test_frame_diff.py && uv run plugins/mobile/skills/ios-take-screenshot/tests/test_stitch_screens.py && uv run plugins/mobile/skills/check-mobile-design/tests/test_check_design.py` — the phone-session hook, simulator claim, frame diff, stitch and design check; needs uv
- `node --test plugins/secrets/skills/secrets-manager/tests/*.mjs` — the local secrets engine and adapter contract
- `node --test plugins/creator/skills/upload-tiktok-video/tests/*.mjs plugins/creator/skills/fetch-tiktok-stats/tests/*.mjs plugins/creator/skills/fetch-tiktok-sounds/tests/*.mjs plugins/creator/skills/open-tiktok-account/tests/*.mjs` — account pick, args, stats and sound rows
- `node --test plugins/computer-use/skills/capture-window/tests/*.mjs` — the window capture
- `cargo test --manifest-path agents/Cargo.toml` — the `agents` dashboard

## Version control

- Bump the plugin's version in the same commit as the change; keep its manifest versions equal.
- Land every change on main through a pull request whose CI passes, then update the installed plugin; main rejects direct pushes, force pushes and deletion.

## Tools

### Install

> Install, update and uninstall plugins at user scope only.

- `claude plugin marketplace update blockchainian` — fetches the pushed catalog
- `claude plugin update <plugin>@blockchainian` — installs the bumped version

### Set up

- Add a new plugin dir to Codex `writable_roots` in `~/.codex/config.toml`.
