# AGENTS.md

> Before working in a plugin, read its `README.md`.  
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
- **tests**: `npm run test:<plugin>`

## Tests

> Run the relevant tests.

- `npm test` — every suite
- `npm run test:marketplace` — the marketplace manifest and shared-engine checks
- `npm run test:codex` — codex-manager against a fake app-server daemon
- `npm run test:feature` — the feature plugin's plan checkers, workstream.sh, watch-ci and retro
- `npm run test:proxy` — the capture logic and WireGuard key derivation
- `npm run test:intel` — the intel skills; the render suites need Chrome and uv
- `npm run test:web` — the heap-snapshot diff and the design check; needs uv
- `npm run test:mobile` — the phone-session hook, simulator claim, frame diff, stitch and design check; needs uv
- `npm run test:secrets` — the local secrets engine and adapter contract
- `npm run test:creator` — account pick, args, stats and sound rows
- `npm run test:computer-use` — the window capture
- `npm run test:agents` — the `agents` dashboard

## Version control

- Bump the plugin's version in the same commit as the change; keep its manifest versions equal.
- Commit and push to main, then update the installed plugin.

## Tools

### Install

> Install, update and uninstall plugins at user scope only.

- `claude plugin marketplace update blockchainian` — fetches the pushed catalog
- `claude plugin update <plugin>@blockchainian` — installs the bumped version

### Set up

- Add a new plugin dir to Codex `writable_roots` in `~/.codex/config.toml`.
