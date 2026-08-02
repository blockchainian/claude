# claude

The `blockchainian` plugin marketplace for Claude Code, and the plugins it
ships.

| Plugin | What it does |
|---|---|
| [codex](plugins/codex/README.md) | Delivers a planned feature as parallel codex workstreams, off Claude's critical path — merged onto your session branch, codex-reviewed, pushed. |
| [grok](plugins/grok/README.md) | Runs the local Grok CLI from Claude Code for read-only reviews and delegated coding tasks. |

## Install

```
/plugin marketplace add blockchainian/claude
/plugin install codex@blockchainian
/plugin install grok@blockchainian
```

Claude Code registers one marketplace per name, so adding this repository is
the only step; both plugins are then installable from it. Each plugin's own
README covers its requirements and usage.

## Layout

```
.claude-plugin/marketplace.json   the catalog, listing both plugins
plugins/codex/                    the codex plugin
plugins/grok/                     the grok plugin
tests/                            grok's node suite
```

Both plugins pin a `version` in their `plugin.json`, which is what Claude Code
compares to decide whether a user is out of date. Bump it in the same commit as
the change you want to ship — a plugin whose version is unchanged stays cached
on every machine that already has it, however much its code moved.

## Test

```
npm test              # both suites
npm run test:codex    # the execute engine against a stub codex CLI
npm run test:grok     # the grok command and runtime suite
npm run validate      # the marketplace and plugin manifests
```

## License

MIT
