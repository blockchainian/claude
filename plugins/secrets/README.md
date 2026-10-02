# secrets

Local plaintext account credentials and browser sessions, outside the repository. The
`secrets-manager` skill imports Google, X and TikTok credentials, drives Camoufox logins,
and lets external adapters define app login, verification and token exports.

## Setup

Install `secrets@blockchainian`, then after every install or update run:

```sh
"${CLAUDE_PLUGIN_ROOT}/skills/secrets-manager/scripts/setup.sh"
"${CLAUDE_PLUGIN_ROOT}/skills/secrets-manager/scripts/setup.sh" --check
```

Setup runs npm install, fetches Camoufox and writes a two-line launcher at
`~/.local/bin/secrets-manager` pointing at this install. Add `~/.local/bin` to PATH.
Node with `node:sqlite` support is required; headed macOS window placement uses Swift.
State defaults to `~/.config/secrets-manager` (`SECRETS_MANAGER_STATE_PATH` overrides it).
`~/.config/secrets-manager/.env` loads without overriding existing environment values:
`RESIDENTIAL_PROXY_URL`, optional `HERO_SMS_API_KEY`, optional `CAPSOLVER_API_KEY`, and
TikTok's `ISP_PROXY_URL` / `ISP_PROXY_COUNT`. No env file ships in this plugin.

## Commands

| Command | Purpose |
|---|---|
| `import <google\|x\|tiktok> [files...]` | Import credentials. |
| `login <google\|x\|tiktok\|app>` | Store a browser session. |
| `login <app> --by-email` | Call an adapter's email hook. |
| `verify <app>` | Probe and persist session status. |
| `setup-2fa` | Enroll Google TOTP, turn on 2-Step, mint an app password. |
| `sms <balance\|prices\|number>` | Manage verification SMS. |
| `export-env <app> [--out FILE]` | Report token presence; optionally write tokens. |
| `get <app> <id>` | Read a stored session. |
| `set-status <app> <id> <status>` | Set session status. |
| `list [--json]` | List accounts and all stored app tables. |
| `validate <adapters.mjs>...` | Load modules with the real kit and print validated names. |

Account commands accept repeatable `--only`, `--all`, `--limit`, `--concurrency`,
`--headed`, and `--rotate-proxy` as described in the skill.
X token verification lives in intel's `fetch-x-mentions/scripts/verify-x.mjs`.

## Adapter interface

The interface lives in `skills/secrets-manager/scripts/adapter.mjs` and is versioned by
this plugin: any field or kit change bumps the minor version; removal or rename bumps major.
An ES module default-exports `(kit) => Adapter[]`. It never imports plugin files by path.

| Field | Contract |
|---|---|
| `name` | Required `[a-z0-9_]+` table and CLI target; `google`, `x` and `tiktok` are reserved. |
| `domain` | Required registrable domain, scopes exports to it and subdomains. |
| `startUrl` | Required login entry URL. |
| `entryTexts` | Required nonempty string array of logged-out entry labels. |
| `signIn(page)` | Required async hook, opens same-tab or popup Google OAuth. |
| `ready(page)` | Required async boolean hook, session token present and live. |
| `signedInUrl(url)` | Optional boolean hook indicating already signed in. |
| `attempts` | Optional positive retry count, default 1. |
| `byEmail(ctx)` | Optional password signup/login through a Gmail plus-alias. |
| `verify(ctx)` | Optional hook returning `active`, `restricted` or `expired`. |
| `exportEnv` | Optional `{envVar, token(session)}`; token returns string or null. |
| `blockedHosts` | Optional host wildcard strings merged at startup. |
| `blockedWebSockets` | Optional WebSocket URL wildcards merged at startup. |

`ByEmailContext` is `{db, cred, opts, io}`; `ByEmailResult` is
`{status: "ok" | "error", alias?, detail?}`. The adapter owns alias selection,
password minting and session saves; the engine registers a successful alias account.
`VerifyContext` is `{db, email, session, opts, io}`. The engine persists a valid result;
a thrown error leaves status unchanged and fails the command.

The kit provides:

- `clickFirst(page, texts, timeout)`, `hasLsKey(page, substr)`, `hasCookie(page, name)`,
  `gotoWithRetry(page, url)` (also under `kit.page`).
- `debug.capture(page, label, tag)`; `restriction.storedTokenLive`, `restriction.tokenLive`.
- `aliasFor(baseEmail, tag)` (required tag), `mintAppPassword(cred, {name, ...})`
  (required name), and `emailOtp.waitForCode(...)`.
- `store`: `openDb`, `getSession`, `saveSession`, `setSessionStatus`, `listAccounts`,
  `sessionsForAccount`, `STATUS_*` and the remaining store exports.
- `config`: `dbPath`, `statePath`, `defaultProxy`, `proxyFor` and the remaining config exports.

Configure `~/.config/secrets-manager/config.json`:

```json
{"adapters": ["/absolute/path/adapters.mjs"]}
```

`SECRETS_MANAGER_ADAPTERS` overrides it with colon-separated absolute paths. No config
and no override means zero adapters. Unknown login/verify/export targets fail with loaded
names; duplicate names or invalid modules fail startup and identify the module path.
`validate` takes module paths directly and exits 0 printing names or 1 reporting the error.
Existing app tables retain their names and remain visible through list/get without adapters.
The plugin owns fixed google/x/tiktok schema and creates app session tables on first save.

## Tests

Run `npm run test:secrets` from the marketplace root. The offline suite uses invented
fixture adapters and exercises every hook; live browser selectors require headed checks.
