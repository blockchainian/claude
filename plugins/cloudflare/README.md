# cloudflare

Cloudflare skills and five Cloudflare MCP servers (API, docs, bindings, builds,
observability) for Claude Code and Codex. Claude Code also provides the
`/build-agent` and `/build-mcp` commands.

This plugin carries the [cloudflare/skills](https://github.com/cloudflare/skills)
plugin (Apache-2.0, version pinned in `plugin.json`) with five of its eleven skills
removed. Claude Code cannot hide individual plugin skills through `skillOverrides`,
so a plugin that ships only the wanted skills is the way to keep the rest out of
every session's context.

## Skills

| Skill | What it does |
|---|---|
| `agents-sdk` | Stateful agents, workflows, and MCP servers on the Agents SDK |
| `cloudflare` | The platform: Workers, Pages, KV, D1, R2, AI, networking, security, IaC |
| `durable-objects` | Durable Objects: RPC, SQLite storage, alarms, WebSockets |
| `web-perf` | Core Web Vitals audits through Chrome DevTools MCP |
| `workers-best-practices` | Production review of Workers code and `wrangler.jsonc` |
| `wrangler` | The Workers CLI for deploys, dev, and resource management |

Not included: Cloudflare One, Cloudflare One migrations, Email Service, Sandbox
SDK, Turnstile. Install the upstream plugin for those.

## Install

### Claude Code

```
/plugin marketplace add blockchainian/claude
/plugin install cloudflare@blockchainian
```

### Codex

See [Codex compatibility](../../docs/codex-compatibility.md) for installation
and MCP authentication. Both hosts use the same six skills and references.

In Codex, use the `agents-sdk` skill to build agents or MCP servers; the Claude
slash commands are convenience entry points and are not duplicated as skills.
`web-perf` additionally requires a configured Chrome DevTools MCP server; the
five bundled Cloudflare servers do not provide browser performance tools.
