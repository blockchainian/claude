# cloudflare

Cloudflare from Claude Code: the five Cloudflare MCP servers (API, docs,
bindings, builds, observability), the `/build-agent` and `/build-mcp` commands,
and the six Cloudflare skills below.

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

```
/plugin marketplace add blockchainian/claude
/plugin install cloudflare@blockchainian
```
