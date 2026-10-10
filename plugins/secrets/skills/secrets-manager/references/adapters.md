# Adapter interface

An adapter file is an ES module that default-exports `(kit) => Adapter[]`. It takes everything it
needs from `kit` and never imports plugin files by path. List the file in
`~/.config/secrets-manager/config.json` and check it with
`node "$SKILL_DIR/scripts/cli.mjs" validate <path>`. The interface is defined in this skill's
`scripts/adapter.mjs`: any change to a field or to the kit bumps the plugin's minor version, and a
removal or rename bumps the major.

## Fields

| Field | Contract |
|---|---|
| `name` | Required. The table and CLI target: `[a-z0-9_]+`, equal to `store.toAppSlug(name)`; `google`, `x`, `tiktok` and the `sqlite_` prefix are reserved. |
| `domain` | Required. The registrable domain; exports are scoped to it and its subdomains. |
| `startUrl` | Required. The login entry URL. |
| `entryTexts` | Required. A nonempty array of the logged-out entry labels. |
| `signIn(page)` | Required async hook that opens Google OAuth, in the same tab or a popup. |
| `ready(page)` | Required async hook: `true` when a live session token is present. |
| `signedInUrl(url)` | Optional: `true` when the URL shows the account is already signed in. |
| `attempts` | Optional positive retry count; default 1. |
| `byEmail(ctx)` | Optional password signup or login through a Gmail plus-alias. |
| `verify(ctx)` | Optional hook returning `active`, `restricted` or `expired`. |
| `setup(ctx)` | Optional async hook returning `{summary, state}`. |
| `setupFlags` | Optional flags for this app's `setup` command. |
| `bannedResponse({url, status, body})` | Optional; returns a nonempty reason when an app-domain response (status >= 400) during `login <app>` means the app banned the account. |
| `whoami({credential})` | Optional async hook returning `{email: string}` for a credential of the app's own type; throws on a missing or ambiguous identity. |
| `credentials(session)` | Optional: returns the session's usable credential fields as `Record<string, string>`, or `null`. |
| `blockedHosts` | Optional host wildcards added to the request blocklist. |
| `blockedWebSockets` | Optional WebSocket URL wildcards added to the blocklist. |

## Hooks

`byEmail` receives `{db, cred, opts, io}` and returns `{status: "ok" | "error", alias?, detail?}`. The
hook picks the alias, mints any password and saves the session itself; when the status is `ok`, the
engine registers the alias account.

`verify` receives `{db, email, session, opts, io}`. It throws when the check cannot tell, and the
status then stays unchanged.

`setup` receives `{db, email, session, opts, io}`, with `session.state` holding the previous run's
state, parsed, or `null`. It resolves to `{summary, state}`, where `summary` is a nonempty one-line string
and `state` is any JSON-serialisable value except `undefined`. On success the engine saves `state` and marks
the session `ready`. When a person must act, throw `kit.NeedsHuman` with a message saying what to do,
and the session becomes `escalated`. Any other error leaves the status and the previous state as they
were.

`setupFlags` maps kebab-case flag names to `{type: "boolean" | "string", description: string}`, with
nonempty descriptions; the names may not be the engine's global flag names or `help`. Values reach `setup`
under their original names:

```js
setupFlags: {
  "follow-lowest-ranked": { type: "boolean", description: "Follow the lowest ranked account." },
}
// in setup: opts["follow-lowest-ranked"] === true
```

## Kit

- `NeedsHuman` is the error to throw when the current step needs a person, with a message that says
  what to do. It stops the app login's retries, but it does not pause the browser or resume the hook.
- `clickFirst(page, texts, timeout = 15000, misses = [])`, `hasLsKey(page, substr)`,
  `hasCookie(page, name)` and `gotoWithRetry(page, url)`, also under `kit.page`. `clickFirst` returns
  a boolean and, for each click timeout, appends `{text, reason}` to `misses`, where `reason` is the
  trimmed Playwright message capped at 2000 characters.
- `debug.capture(page, email, label, note)` saves a screenshot, plus `note` as `screenshot.txt`;
  it never throws. `restriction.storedTokenLive` and `restriction.tokenLive` check a token.
- `aliasFor(baseEmail, tag)` and `mintAppPassword(cred, {name, ...})`; `tag` and `name` are required.
- `withProfile(key, {headed = false, rotate = false, proxyUrl = config.proxyFor(key, {rotate}),
  blockAssets = true, record = false}, fn)` opens the account's persistent Camoufox profile for a
  `byEmail`, `verify` or `setup` hook, with the engine's proxy, fingerprint and request blocking. It
  calls `fn(context, page)`, returns its result and always closes the context; `record` saves page
  videos of a headless run under the account's debug dir.
- `ispFetch(url, init)` is `fetch` through a random ISP pool slot with a Firefox TLS fingerprint, for
  a `verify` hook that checks the account over the app's own API. It needs `ISP_PROXY_URL`.
- For a hook that drives a login itself: `gotoPastCloudflare(page, url, {assist = false,
  timeoutMs = 45000})` waits out a Cloudflare challenge, assisting when headed;
  `waitReady(page, adapter, timeoutMs = 40000)` polls `adapter.ready`;
  `appAlreadySignedIn(page, adapter, timeoutMs = 12000)`;
  `withAppRetries(attempts, attempt, {onRetry})`; `exportScoped(db, page, adapter, email)` saves the
  session scoped to `adapter.domain`; and `filterState(state, domain)` does the same scoping on a
  state object.
- `emailOtp.extractOtp(text)`, `emailOtp.otpCandidates(text)` and
  `emailOtp.readSignupOtp(baseEmail, appPassword, {toAlias = null, sinceEpoch = null, timeoutS = 120,
  pollS = 5} = {})`. `readSignupOtp` reads the base Gmail inbox with its app password, optionally
  only mail to a plus-alias and newer than `sinceEpoch` (seconds), and checks Spam once the inbox
  times out. It returns `{otp, from, subject, to, folder}`, the same with
  `{securityAlert: true, otp: null}` for a Security Alert, or `null` on timeout.
- `store` (`openDb`, `getSession`, `saveSession`, `saveSetupState`, `setSessionStatus`,
  `listAccounts`, `sessionsForAccount`, `STATUS_*`, …), `config` (`dbPath`, `statePath`,
  `defaultProxy`, `proxyFor`, …) and `credentials` (`loadCredentials(dir)`,
  `setAppPassword(dir, email, appPassword)`, `setTotpSecret(dir, email, secret)`). `login` reads the
  credential files under `config.credentialsDir(app)` on every run, so a hook that mints an app
  password must write it back there.
