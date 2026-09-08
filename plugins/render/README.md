# render

Render from Claude Code: the Render MCP server over OAuth, the `render-assistant`
agent, the `/check-render-status` command, a `PostToolUse` hook that validates
`render.yaml` on every edit, and the nine Render skills below.

This plugin carries the [render-oss/render-plugin-claude-code](https://github.com/render-oss/render-plugin-claude-code)
plugin (MIT, version pinned in `plugin.json`) with twelve of its twenty-one skills
removed. Claude Code cannot hide individual plugin skills through `skillOverrides`,
so a plugin that ships only the wanted skills is the way to keep the rest out of
every session's context. Skills come from [render-oss/skills](https://github.com/render-oss/skills).

## Skills

| Skill | What it does |
|---|---|
| `render-blueprints` | Author and validate `render.yaml` Blueprints |
| `render-cli` | Install and use the Render CLI for deploys, logs, SSH, psql |
| `render-debug` | Diagnose failed deploys from logs, metrics, and database state |
| `render-disks` | Persistent disks: mount paths, sizing, snapshots |
| `render-env-vars` | Environment variables, secrets, and env groups |
| `render-keyvalue` | Render Key Value (Redis-compatible) instances |
| `render-mcp` | Set up and troubleshoot the Render MCP server |
| `render-monitor` | Service health, metrics, logs, and deploy status |
| `render-postgres` | Managed PostgreSQL: connections, backups, replicas |

Not included: background workers, cron jobs, deploy, Docker, domains, Heroku
migration, networking, private services, scaling, static sites, web services,
workflows. Install the upstream plugin for those.

## Install

```
/plugin marketplace add blockchainian/claude
/plugin install render@blockchainian
```

The MCP server authenticates over OAuth on first use; no API key is needed.
