# Codex compatibility

This change adds Codex support to Cloudflare, Web, Proxy, Creator, and Render
while keeping their skills and scripts shared with Claude Code.

## Scope

- Add a Codex marketplace containing only these five plugins.
- Resolve script paths from the installed skill rather than a Claude-only variable.
- Adapt browser and background-process instructions to the host's available tools.
- Verify MCP configuration and keep Claude-specific hooks and agents explicitly scoped.
- Preserve existing business scripts, state locations, and Claude installation behavior.

Feature, Codex bridge, Intel, Secrets, and Mobile compatibility are outside this
change. Creator continues to use the existing secrets-manager state; this does
not port the Secrets plugin or recreate accounts.

## Validation

Run the existing marketplace, Web, Proxy, and Creator checks, validate both plugin
formats, and exercise Codex discovery and installation in an isolated temporary
home. Record any live authentication or device-dependent checks that could not
be performed without changing the user's environment.
