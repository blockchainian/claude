# AGENTS.md

> Before working in a component, read its `AGENTS.md` and `README.md`.
> Never hard-code a path of this machine: the repo is public.
> Never import a plugin script by its versioned cache path.

This repo is the `blockchainian` plugin marketplace for Claude Code and Codex, and the only home of all its plugins.

## Modules

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

- **validate**: `claude plugin validate --strict .`
- **tests**: the plugin's own test command

## Tools

### Install

> Install, update and uninstall plugins at user scope only.

- `claude plugin marketplace update blockchainian` — fetches the pushed catalog
- `claude plugin update <plugin>@blockchainian` — installs the bumped version

### Set up

- Add a new plugin dir to Codex `writable_roots` in `~/.codex/config.toml`.

## Version control

- Bump the plugin's version in the same commit as the change; keep its manifest versions equal.
- Land every change on main through a pull request whose CI passes, then update the installed plugin; main rejects direct pushes, force pushes and deletion.