# AGENTS.md

> Before working in a plugin, read its `README.md`.  
> Never hard-code a path of this machine: the repo is public.  
> Never import a plugin script by its versioned cache path.

This repo is the `blockchainian` plugin marketplace for Claude Code and Codex, and the only home of all its plugins.

## Modules

- .claude-plugin/ -- the Claude Code catalog
- .agents/plugins/ -- the Codex catalog
- plugins/ -- one directory per plugin
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

- Run the relevant tests.

## Version control

- Bump the plugin's version in the same commit as the change; keep its manifest versions equal.
- Commit and push to main, then update the installed plugin.

## Tools

### Install

- `claude plugin marketplace update blockchainian` — fetches the pushed catalog
- `claude plugin update <plugin>@blockchainian` — installs the bumped version

### Set up

- Add a new plugin dir to Codex `writable_roots` in `~/.codex/config.toml`.
