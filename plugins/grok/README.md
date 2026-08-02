# grok

Use Grok from inside Claude Code for code reviews or to delegate tasks to Grok.

This plugin is for Claude Code users who want to use the local `grok` CLI from the workflow they already have.

## What You Get

- `/grok:review` for a read-only Grok review of your current changes or branch diff
- `/grok:rescue` to delegate a focused coding task to Grok through the `grok:grok-rescue` subagent

## Requirements

- **Grok CLI installed and signed in**
- **Node.js 18.18 or later**

## Install

Install the Grok CLI:

```bash
curl -fsSL https://x.ai/cli/install.sh | bash
```

Restart your shell if needed, then verify that `grok` is available:

```bash
grok --version
```

Sign in to Grok:

```bash
grok login
```

Add the marketplace in Claude Code:

```bash
/plugin marketplace add blockchainian/claude
```

Install the plugin:

```bash
/plugin install grok@blockchainian
```

Reload plugins:

```bash
/reload-plugins
```

After install, you should see:

- `/grok:review` and `/grok:rescue` in slash command autocomplete
- the `grok:grok-rescue` subagent in `/agents`

One simple first run is:

```bash
/grok:review
```

If your working tree has no changes, it should report that there are no changes to review.

## Usage

### `/grok:review`

Runs a read-only Grok review on your current work.

Use it when you want:

- a review of your current uncommitted changes
- a review of your branch compared to a base branch like `main`
- a review focused on a specific risk area

Use `--base <ref>` for branch review. Any text after the flags is passed to Grok as reviewer focus.

Examples:

```bash
/grok:review
/grok:review --base main
/grok:review --base main focus on security and data loss risks
/grok:review focus on missing tests
```

This command gathers git status and diff context locally, then invokes the local `grok` CLI with a temporary prompt file. It asks Grok not to edit files and disables Grok tools for the review run.

### `/grok:rescue`

Hands a task to Grok through the `grok:grok-rescue` subagent.

Use it when you want Grok to:

- investigate a bug
- try a focused fix
- take a second pass on a failing test or implementation detail
- report what files it changed and what verification it ran

Examples:

```bash
/grok:rescue investigate why the tests started failing
/grok:rescue fix the failing test with the smallest safe patch
/grok:rescue review the auth helper and suggest a safer implementation
```

The subagent uses your local `grok` CLI. If Grok is unavailable or not signed in, it reports the exact error.

## Typical Flows

### Review Before Shipping

```bash
/grok:review --base main
```

### Hand A Problem To Grok

```bash
/grok:rescue investigate why the build is failing
```

## Grok Integration

This plugin uses the global `grok` binary installed in your environment.

That means:

- it uses the same Grok install you would use directly
- it uses the same local Grok authentication state
- it runs from the same repository checkout and machine-local environment
- it does not install Grok for you
- it does not manage Claude Code configuration after the plugin is installed

For Grok CLI help and configuration options, run:

```bash
grok --help
```

## Local Development

From the repository root, validate the plugin without installing it:

```bash
npm run test:grok
claude plugin validate --strict plugins/grok
```

Load the plugin for one Claude Code session without changing installed plugins:

```bash
claude --no-session-persistence --plugin-dir plugins/grok
```

For a non-interactive smoke test:

```bash
claude --no-session-persistence --plugin-dir plugins/grok -p "Say ok" --tools ""
```

## FAQ

### Is `blockchainian/claude` a GitHub repository?

Yes. `claude plugin marketplace add` accepts a URL, local path, or GitHub repository source. The `owner/repo` form follows the GitHub organization or username and repository name, so this plugin uses:

```bash
/plugin marketplace add blockchainian/claude
```

### Why is the install command `grok@blockchainian`?

`grok` is the plugin name. `blockchainian` is the marketplace name declared by this repository, which ships this plugin alongside [codex](../codex/README.md).

Claude Code uses `plugin@marketplace` when installing a plugin from a specific marketplace.

### Do I need a separate Grok account for this plugin?

You need whatever account or authentication method your local `grok` CLI requires. If `grok` works in your terminal, the plugin should use the same local authentication.

### Does the plugin use a separate Grok runtime?

No. It delegates through your local `grok` CLI on the same machine.

### Can I test the plugin without installing it?

Yes. Use `--plugin-dir plugins/grok` from this checkout as shown in [Local Development](#local-development).
