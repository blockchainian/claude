# secrets

Shared by Claude Code and Codex: local plaintext account credentials and browser sessions, outside the repository. The
`secrets-manager` skill imports Google, X and TikTok credentials, drives Camoufox logins,
and lets external adapters define app login, verification, account setup and credential exports.

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

Start a new session after installing or updating. Both hosts load the same
`secrets-manager` skill, CLI and adapter interface. Each install has its own npm
dependencies; both read the same existing state at `~/.local/state/secrets-manager`.
Installing in another host does not create or migrate accounts. Close a profile's
browser before opening the same account from another host.

## Setup

Install `secrets@blockchainian`. Set `SKILL_DIR` to the absolute directory containing
the installed secrets-manager `SKILL.md`, then after every install or update run:

```sh
SKILL_DIR="/absolute/path/to/loaded/secrets-manager"
"$SKILL_DIR/scripts/setup.sh"
"$SKILL_DIR/scripts/setup.sh" --check
```

Setup runs npm install and fetches Camoufox. Invoke `node "$SKILL_DIR/scripts/cli.mjs"`
directly from the installed skill; setup does not install a global launcher.
Node with `node:sqlite` support is required; headed macOS window placement uses Swift.
Accounts and profiles default to `~/.local/state/secrets-manager` (`SECRETS_STATE_DIR` overrides it); debug captures
and scratch to `~/.local/share/secrets-manager` (`SECRETS_DATA_DIR`).
`~/.config/secrets-manager/.env` loads without overriding existing environment values:
`RESIDENTIAL_PROXY_URL`, optional `HERO_SMS_API_KEY`, and
TikTok's `ISP_PROXY_URL` / `ISP_PROXY_COUNT`. No env file ships in this plugin.

## Commands

| Command | Purpose |
|---|---|
| `import <google\|x\|tiktok> [files...]` | Import credentials. |
| `login <google\|x\|tiktok\|app>` | Store a browser session. |
| `login <app> --by-email` | Call an adapter's email hook. |
| `verify <google\|x\|tiktok\|app>` | Check each account is still usable and persist its status. |
| `setup <app>` | Prepare logged-in accounts through the adapter hook and mark them `ready`. |
| `setup-2fa` | Enroll Google TOTP, turn on 2-Step, mint an app password. |
| `sms <balance\|prices\|number>` | Manage verification SMS. |
| `whoami <x\|app> --select CREDENTIAL [--json]` | Identify a credential without reading or changing stored accounts. |
| `export <app> [--select EMAIL]...` | Print active or ready session credentials as JSONL. |
| `get <app> --select ID...` | Read stored sessions. |
| `set-status <app> <status> --select ID...` | Set session status. |
| `list [--json]` | List accounts and all stored app tables. |
| `validate <adapters.mjs>...` | Load modules with the real kit and print validated names. |

Account commands accept repeatable `--select`, `--all`, `--limit`, `--concurrency`,
`--headed` (Google sign-ins are headed by default; `--headless` opts out), and `--rotate-proxy`
as described in the skill.
`verify google|x|tiktok` are builtin checks; any other target calls the adapter's `verify` hook.
Deriving a ct0 for a vendor X auth_token stays in intel's `fetch-x-mentions/scripts/verify-x.mjs`.

`setup <app> [--select EMAIL]... [--all] [--concurrency N] [--headed] [--rotate-proxy] [app flags]`
selects imported accounts with `active` app sessions; `--all` adds `ready` sessions for a rerun.
The hook receives `{db, email, session, opts, io}` and resolves to `{summary, state}`.
`summary` must be a nonempty one-line string; `state` is any JSON-serialisable app-specific
value, including `null`, but not `undefined`. Success stores the state as JSON in
`state` and status `ready` in one write, then prints `<email>: <summary>`. A throw or
invalid result, summary or state prints `<email>: <message>` to stderr, writes nothing,
keeps status and prior setup state, and exits 1 while other accounts continue.
A missing hook fails with `<app> has no setup hook`.

Adapters may declare `setupFlags`, an object mapping kebab-case flag names to
`{type: "boolean" | "string", description: string}`. Descriptions must be nonempty;
engine global flag names and `help` are reserved. For example:

```js
setupFlags: {
  "follow-lowest-ranked": { type: "boolean", description: "Follow the lowest ranked account." },
}
```

Only `setup <app>` accepts that app's flags; other apps, commands and unknown flags
fail before the setup hook runs or a browser opens. Values reach the hook under their
original names, e.g. `opts["follow-lowest-ranked"] === true`. `setup <app> --help`
and flag errors list the app's flags with descriptions. The hook's `session.state`
is already parsed JSON from the previous setup, or `null` when absent.

`ready` means logged in and app setup done, a step above `active`. Default verification and
credential export accept both. Re-login and an `active` verification result preserve `ready`
because setup lives server-side; expired/restricted results still invalidate usability.
Old Google and app tables are widened on open, preserving rows; only app setup promotes to `ready`.
App tables have a nullable `state TEXT` column containing JSON. Existing stores add it
on open with guarded, idempotent `ALTER TABLE ... ADD COLUMN`, without rebuilding for this
column. `get <app> --select EMAIL` shows parsed `state`; re-login leaves it untouched.

`export` prints one JSON line per selected active or ready session:
`{"app":"<app>","email":"<email>",...fields}`. Credential values are printed in clear.
A null hook result prints `<email>\tmissing` to stderr; stdout contains only JSONL for jq.
An adapter without the hook fails with `<app> has no credentials hook`.

## Adapter interface

See `skills/secrets-manager/references/adapters.md`.

## Tests

Run `npm run test:secrets` from the marketplace root. The offline suite uses invented
fixture adapters and exercises every hook; live browser selectors require headed checks.

## Environment Variables

Runtime configuration is `~/.config/secrets-manager/.env`; start from this plugin’s empty `.env.example`. The skill’s **Environment Variables** table lists the settings. Account rows, credential files and browser profiles remain in the existing state directory.
