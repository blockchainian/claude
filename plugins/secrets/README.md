# secrets

Local plaintext account credentials and browser sessions, outside the repository. The
`secrets-manager` skill imports Google, X and TikTok credentials, drives Camoufox logins,
and lets external adapters define app login, verification and credential exports.

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
`RESIDENTIAL_PROXY_URL`, optional `HERO_SMS_API_KEY`, and
TikTok's `ISP_PROXY_URL` / `ISP_PROXY_COUNT`. No env file ships in this plugin.

## Commands

| Command | Purpose |
|---|---|
| `import <google\|x\|tiktok> [files...]` | Import credentials. |
| `login <google\|x\|tiktok\|app>` | Store a browser session. |
| `login <app> --by-email` | Call an adapter's email hook. |
| `verify <google\|x\|tiktok\|app>` | Check each account is still usable and persist its status. |
| `setup-2fa` | Enroll Google TOTP, turn on 2-Step, mint an app password. |
| `sms <balance\|prices\|number>` | Manage verification SMS. |
| `whoami <app> --select CREDENTIAL [--json]` | Identify a credential without reading or changing stored accounts. |
| `export <app> [--select EMAIL]...` | Print active-session credentials as JSONL. |
| `get <app> --select ID...` | Read stored sessions. |
| `set-status <app> <status> --select ID...` | Set session status. |
| `list [--json]` | List accounts and all stored app tables. |
| `validate <adapters.mjs>...` | Load modules with the real kit and print validated names. |

Account commands accept repeatable `--select`, `--all`, `--limit`, `--concurrency`,
`--headed` (Google sign-ins are headed by default; `--headless` opts out), and `--rotate-proxy`
as described in the skill.
`verify google|x|tiktok` are builtin checks; any other target calls the adapter's `verify` hook.
Deriving a ct0 for a vendor X auth_token stays in intel's `fetch-x-mentions/scripts/verify-x.mjs`.

`export` prints one JSON line per selected active session:
`{"app":"<app>","email":"<email>",...fields}`. Credential values are printed in clear.
A null hook result prints `<email>\tmissing` to stderr; stdout contains only JSONL for jq.
An adapter without the hook fails with `<app> has no credentials hook`.

## Adapter interface

The interface lives in `skills/secrets-manager/scripts/adapter.mjs` and is versioned by
this plugin: any field or kit change bumps the minor version; removal or rename bumps major.
An ES module default-exports `(kit) => Adapter[]`. It never imports plugin files by path.

| Field | Contract |
|---|---|
| `name` | Required normalized `[a-z0-9_]+` table and CLI target (must equal `store.toAppSlug(name)`); `google`, `x`, `tiktok` and the `sqlite_` prefix are reserved. |
| `domain` | Required registrable domain, scopes exports to it and subdomains. |
| `startUrl` | Required login entry URL. |
| `entryTexts` | Required nonempty string array of logged-out entry labels. |
| `signIn(page)` | Required async hook, opens same-tab or popup Google OAuth. |
| `ready(page)` | Required async boolean hook, session token present and live. |
| `signedInUrl(url)` | Optional boolean hook indicating already signed in. |
| `attempts` | Optional positive retry count, default 1. |
| `byEmail(ctx)` | Optional password signup/login through a Gmail plus-alias. |
| `verify(ctx)` | Optional hook returning `active`, `restricted` or `expired`. |
| `bannedResponse({url, status, body})` | Optional; called during `login <app>` for every app-domain response with status >= 400. A non-empty reason means the app banned the account: its session row is recorded `restricted` (created if absent), the Google account is untouched, and `login <app>` never retries it. |
| `whoami({credential})` | Optional async hook returning `{email: string}`; credential type is app-specific. Missing or ambiguous identity throws. |
| `credentials(session)` | Optional `(session) => Record<string, string> \| null`; returns usable credential fields. |
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
  (required name).
- `withProfile(key, {headed = false, rotate = false, proxyUrl = config.proxyFor(key, {rotate}),
  blockAssets = true, record = false}, fn)` opens a persistent Camoufox browser for a `byEmail` or `verify`
  hook. It reuses the engine's profile directory, proxy, stored fingerprint, traffic blocking,
  headed diagnostics and cleanup; a headless run with `record` saves page videos under the
  account's debug dir. Calls `fn(context, page)`, returns its result,
  and closes the context in `finally`, including when the callback throws.
  Resolve proxy URLs explicitly with `kit.config.proxyFor(key, {rotate})` when needed.
- `ispFetch(url, init)` is `fetch` through a random ISP pool slot (`ISP_PROXY_URL`, slots
  1..`ISP_PROXY_COUNT`) with a Firefox TLS fingerprint (impit), like Camoufox's,, for a `verify` hook that checks
  an account over the app's own API. Returns a fetch `Response`; it throws without `ISP_PROXY_URL`.
- The engine's app-flow steps, for a hook that drives a login itself:
  `gotoPastCloudflare(page, url, {assist = false, timeoutMs = 45000})` (waits out a
  Cloudflare challenge, assisting when headed), `waitReady(page, adapter, timeoutMs = 40000)`
  (polls `adapter.ready`), `appAlreadySignedIn(page, adapter, timeoutMs = 12000)`,
  `withAppRetries(attempts, attempt, {onRetry})`, `exportScoped(db, page, adapter, email)`
  (saves the session scoped to `adapter.domain`) and the pure `filterState(state, domain)`.
- `emailOtp` exposes the email-otp module's real exports: `extractOtp(text)`,
  `otpCandidates(text)`, and `readSignupOtp(baseEmail, appPassword, {toAlias = null,
  sinceEpoch = null, timeoutS = 120, pollS = 5} = {})`. The reader uses the base Gmail
  inbox and app password (spaces stripped), optionally filters a plus-alias and messages
  newer than the click (`sinceEpoch` in seconds), then checks Spam after the inbox timeout.
  Returns `{otp, from, subject, to, folder}`, or the same metadata with
  `{securityAlert: true, otp: null}` for a Security Alert, or `null` on timeout.
- `store`: `openDb`, `getSession`, `saveSession`, `setSessionStatus`, `listAccounts`,
  `sessionsForAccount`, `STATUS_*` and the remaining store exports.
- `config`: `dbPath`, `statePath`, `defaultProxy`, `proxyFor` and the remaining config exports.
- `credentials`: `loadCredentials(dir)`, `setAppPassword(dir, email, appPassword)`,
  `setTotpSecret(dir, email, secret)` — the credential files under `config.credentialsDir(app)`
  are what `login` reads each run, so a hook that mints an app password writes it back there.

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
