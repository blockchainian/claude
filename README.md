# claude

The `blockchainian` plugin marketplace for Claude Code, and the plugins it
ships.

| Plugin | What it does |
|---|---|
| [codex](plugins/codex/README.md) | Delivers a planned feature as parallel codex workstreams, off Claude's critical path — merged onto your session branch and pushed — and reviews the delta into a findings file. |
| [grok](plugins/grok/README.md) | Runs the local Grok CLI from Claude Code for read-only reviews and delegated coding tasks. |
| [build-ios-apps](plugins/build-ios-apps/README.md) | Builds, runs, profiles, and screenshots iOS apps on a simulator or a connected iPhone. |
| [render](plugins/render/README.md) | The Render plugin with only the nine skills this desk uses, plus its MCP server, agent, and hook. |
| [cloudflare](plugins/cloudflare/README.md) | The Cloudflare plugin with only the six skills this desk uses, plus its five MCP servers. |
| [proxyman](plugins/proxyman/README.md) | Captures and decodes the HTTP and WebSocket traffic of one target web or mobile app with mitmproxy, scoped to that app's hosts. |

## Install

```
/plugin marketplace add blockchainian/claude
/plugin install codex@blockchainian
/plugin install grok@blockchainian
/plugin install build-ios-apps@blockchainian
/plugin install render@blockchainian
/plugin install cloudflare@blockchainian
/plugin install proxyman@blockchainian
```

Claude Code registers one marketplace per name, so adding this repository is
the only step; every plugin is then installable from it. Each plugin's own
README covers its requirements and usage.

## Layout

```
.claude-plugin/marketplace.json   the catalog, listing every plugin
plugins/codex/                    the codex plugin
plugins/grok/                     the grok plugin
plugins/build-ios-apps/           the iOS plugin
plugins/render/                   the Render plugin, trimmed to nine skills
plugins/cloudflare/               the Cloudflare plugin, trimmed to six skills
plugins/proxyman/                 the mitmproxy traffic-capture plugin
tests/                            grok's node suite
```

Every plugin pins a `version` in their `plugin.json`, which is what Claude Code
compares to decide whether a user is out of date. Bump it in the same commit as
the change you want to ship — a plugin whose version is unchanged stays cached
on every machine that already has it, however much its code moved.

## Test

```
npm test              # both suites
npm run test:codex    # implement.sh and review.sh against a stub codex CLI
npm run test:grok     # the grok command and runtime suite
npm run validate      # the marketplace and plugin manifests
```

## License

MIT
