# secrets

Local plaintext account credentials and browser sessions for Claude Code and Codex, kept outside the
repository. The `secrets-manager` skill imports Google, X and TikTok accounts, signs them in through
Camoufox, and lets external adapters define each app's login, verification, account setup and
credential export.

## Install

Claude Code:

```text
/plugin marketplace add blockchainian/claude
/plugin install secrets@blockchainian
```

Codex:

```sh
codex plugin marketplace add blockchainian/claude
codex plugin add secrets@blockchainian
```

Start a new session after installing or updating, then run the skill's setup:

```sh
SKILL_DIR="/absolute/path/to/loaded/secrets-manager"
"$SKILL_DIR/scripts/setup.sh"
"$SKILL_DIR/scripts/setup.sh" --check
```

It needs Node with `node:sqlite` support, and Swift for headed window placement on macOS. Both
hosts read the same accounts, so close a profile's browser before opening that account from the
other host. Configuration goes in `~/.config/secrets-manager/.env`, starting from this plugin's
`.env.example`; the skill's **Environment Variables** table lists the settings.

## Commands

Run `node "$SKILL_DIR/scripts/cli.mjs" <command>`; `--help` gives every flag.

| Command | Purpose |
|---|---|
| `import <google\|x\|tiktok> [files...]` | Import credentials. |
| `login <google\|x\|tiktok\|app>` | Store a browser session. |
| `login <app> --by-email` | Call an adapter's email hook. |
| `verify <google\|x\|tiktok\|app>` | Check each account is still usable and save its status. |
| `setup <app>` | Prepare logged-in accounts through the adapter hook and mark them `ready`. |
| `setup-2fa` | Enroll Google TOTP, turn on 2-Step, mint an app password. |
| `sms <balance\|prices\|number>` | Rent numbers for Google's verification SMS. |
| `whoami <x\|app> --select CREDENTIAL [--json]` | Identify a credential without touching stored accounts. |
| `export <app> [--select EMAIL]...` | Print active or ready session credentials as JSONL. |
| `get <app> --select ID...` | Read stored sessions. |
| `set-status <app> <status> --select ID...` | Set session status. |
| `list [--json]` | List accounts and all stored app tables. |
| `validate <adapters.mjs>...` | Load adapter modules with the real kit and print their names. |

How to use them is in the skill's `SKILL.md`; how to write an adapter is in
`skills/secrets-manager/references/adapters.md`.

## Tests

Run `npm run test:secrets` from the marketplace root. The offline suite exercises every hook with
fixture adapters; live browser selectors need headed checks.
