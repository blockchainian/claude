# Codex compatibility

Cloudflare, Web, Proxy, Creator, and Render share their skills and scripts between
Claude Code and Codex. `.agents/plugins/marketplace.json` exposes only those five
plugins to Codex; `.claude-plugin/marketplace.json` continues to expose all ten to
Claude Code. No other plugin is included in this compatibility change.

## Install

Codex CLI 0.160.0 was used to verify the commands below. Other clients or versions
must support plugin marketplaces and Claude-compatible plugin manifests.

```sh
codex plugin marketplace add blockchainian/claude
codex plugin add cloudflare@blockchainian
codex plugin add web@blockchainian
codex plugin add proxy@blockchainian
codex plugin add creator@blockchainian
codex plugin add render@blockchainian
```

Before this PR merges, use `codex plugin marketplace add blockchainian/claude --ref codex`
instead of the first command. For a local checkout, run `codex plugin marketplace add .`
from its root. Start a new Codex session after installing plugins. To refresh Git
marketplace sources later, use `codex plugin marketplace upgrade blockchainian`,
then verify installed versions with `codex plugin list --marketplace blockchainian`.

Claude Code installation remains unchanged:

```text
/plugin marketplace add blockchainian/claude
/plugin install cloudflare@blockchainian
/plugin install web@blockchainian
/plugin install proxy@blockchainian
/plugin install creator@blockchainian
/plugin install render@blockchainian
```

## Shared paths and dependencies

For skills with scripts, set `SKILL_DIR` to the absolute directory containing the
`SKILL.md` loaded by the host. Use that actual installed path in each shell call;
do not depend on `CLAUDE_PLUGIN_ROOT`, the user's working directory, or a previous
shell's variables. Full plugin installation preserves sibling script imports.

| Plugin | Shared dependencies | Host-specific behavior |
|---|---|---|
| Cloudflare | Five remote Cloudflare MCP servers; `web-perf` additionally needs Chrome DevTools MCP | Claude's build commands remain available; Codex uses `agents-sdk` directly |
| Web | Node 22+, Chrome with a debug port for heap capture, `uv` for image comparison | Screenshot and UI instructions use the host's available browser tools |
| Proxy | macOS, mitmproxy and trusted CA; logged-in Chrome with Zero Omega or an iPhone WireGuard tunnel | User enables browser routing; the script detaches its own proxy hub |
| Creator | Node 22.13+, npm dependencies, Camoufox, existing secrets-manager state and proxy configuration | Shell process handles and completion notifications belong to the host; shown windows and recording need macOS permissions |
| Render | Render MCP authentication; Render CLI for CLI/Blueprint operations | Separate OAuth clients; Claude's agent and edit-validation hook stay Claude-specific; Codex uses `render-monitor` |

Creator uses the existing secrets-manager database, logged-in profiles and ISP
slots. Installing it in Codex does not install Secrets, create accounts, or change
their state. Both hosts must avoid opening the same account profile concurrently.
Creator has no Claude Workflow dependency; `intel/case-study/workflows/creator.mjs`
is a different skill and is outside this change.

## MCP authentication and Render behavior

Cloudflare uses its existing `.mcp.json` in both hosts. Complete each server's
authentication when requested by the host. Installing the plugin does not prove
that an account has authorized every server.

Render's `.mcp.json` uses OAuth client `claude`. Its Codex manifest overrides the
server with OAuth client `codex`, as documented by [Render](https://render.com/docs/mcp-server).
It explicitly selects an empty Codex hook file so Claude's `Edit|Write|MultiEdit`
hook is not loaded. In Codex, use `render-blueprints` to validate YAML explicitly;
there is no automatic validation after an edit. The `render-mcp` skill also covers
manual configuration for clients without plugin OAuth support.

Codex reads the existing Claude manifests for the other four plugins. Only Render
needs a separate manifest; its identity and version stay aligned across both files.
See [OpenAI's plugin packaging documentation](https://developers.openai.com/plugins/build/plugins)
for compatible manifests, marketplace discovery and bundled MCP configuration.

## Validation

- Marketplace regression tests cover the five-plugin boundary and Render's separate
  OAuth clients, aligned versions and host-scoped hooks.
- Existing Web, Proxy and Creator suites cover their unchanged business scripts.
- Codex CLI 0.160.0 discovers and installs all five into a temporary `CODEX_HOME`.
  App-server `plugin/read` reports 22 skills, six MCP servers and no hooks;
  `skills/list` loads all 22 authored plugin skills without parsing errors; this CLI
  also generates one Render status shortcut from the existing command.
- Creator CLI smoke checks run from a relocated path containing spaces, with a
  different working directory and without `CLAUDE_PLUGIN_ROOT`; missing inputs or
  account state fail explicitly, without opening browsers or posting.
- Claude's strict marketplace and individual-plugin validators accept the packages.

Live OAuth login, real TikTok browser/posting flows, Chrome CDP capture, Zero Omega
routing and WireGuard capture were not exercised. They require the user's local
services or account state; successful installation is not a live end-to-end result.
