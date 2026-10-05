---
name: secrets-manager
description: Manage local plaintext credentials and browser sessions, import Google/X/TikTok accounts, log in through external app adapters, check that accounts are still usable (verify), provision Google 2FA, and export app credentials.
---

# Secrets manager

Resolve `SKILL_DIR` from the absolute directory containing this loaded `SKILL.md`,
not the project working directory. Set it in every shell call. Run the CLI directly
from this skill; no global command is installed. All state lives under `SECRETS_MANAGER_STATE_PATH` (default
`~/.config/secrets-manager`); the residential proxy from `~/.config/secrets-manager/.env` is required for every
browser command (the home IP is never used).

## Setup (after every plugin install or update)

```sh
SKILL_DIR="/absolute/path/to/loaded/secrets-manager"
"$SKILL_DIR/scripts/setup.sh"
# Read-only report:
"$SKILL_DIR/scripts/setup.sh" --check
```

Setup installs dependencies and downloads Camoufox; it does not install a global launcher.
Configure external adapters in `~/.config/secrets-manager/config.json`:

```json
{"adapters": ["/absolute/path/adapters.mjs"]}
```

`SECRETS_MANAGER_ADAPTERS` (colon-separated absolute paths) overrides that file.
Run `node "$SKILL_DIR/scripts/cli.mjs" validate /absolute/path/adapters.mjs` to check the real kit contract.
Without a config file or override, no app adapters are loaded; other commands still work.
See the plugin README's Adapter interface for every field and kit helper.

Pinned: Camoufox v152.0.4-beta.30, camoufox-js 0.12.0, playwright-core 1.60.0. Headed runs open the window on the
display `CAMOUFOX_DISPLAY` names in `~/.config/secrets-manager/.env` (any part of its name, any
case, e.g. `SAMSUNG`; unset means the main display; `displayOrigin.swift`), move OAuth popups there
(`moveWindows.swift`, needs Accessibility trust) and record them (`recordWindows.swift`, needs Screen Recording
permission), both through `swift`. Headless Google sign-ins (`login`, `setup-2fa`, app-password
minting) save a Playwright page video per page, OAuth popup included, under
`~/.config/secrets-manager/debug/<email>/rec-<ts>/`.

## New account flow

A fresh Google account signs into the apps your adapters define. Run each step separately:

1. `node "$SKILL_DIR/scripts/cli.mjs" import google <file>`.
2. `node "$SKILL_DIR/scripts/cli.mjs" login google --select <email>`.
3. `node "$SKILL_DIR/scripts/cli.mjs" login <app> --select <email>` for each required adapter.

Every Google sign-in (`login google`, `login <app>`, `setup-2fa`) runs headed by default, so a person
at the window clears any CAPTCHA: the headless vision solver's misses get accounts banned.
`--headless` opts out. `login x|tiktok` and `verify` stay headless unless `--headed`.

Done means Google and every required app session are active; `list` shows their status.

Every step reuses the account's one profile, so an app step never re-does Google. When a step
fails it records a status that says how to proceed:

- `escalated` — a Google challenge the script could not pass. Re-run that step (headed) so a
  person can clear it; a later successful `login google` clears the escalation.
- `restricted` — the app banned the account. Stop for that app; the session is unusable.
- `expired` — the session lapsed. Re-run that step.

A failed step does not halt the others, but an app step needs Google `active` first (an
`escalated` account is skipped by `login <app>`), so fix `login google` before chasing an app
failure. Run one account at a time (`--select`) for a fresh batch so each outcome is
watched; widen to `--all` / higher `--concurrency` once the flow is proven on a few.

## Commands

```
SKILL_DIR="/absolute/path/to/loaded/secrets-manager"
node "$SKILL_DIR/scripts/cli.mjs" <command> [options]

import <google|x|tiktok> [file...]
login <google|app> [--select ID]... [--all] [--limit N] [--concurrency N] [--headless] [--rotate-proxy]
login <x|tiktok> [--select ID]... [--all] [--limit N] [--concurrency N] [--headed] [--rotate-proxy]
login <app> --by-email [--mint-app-password] [--select EMAIL]... [--headed]
verify <google|x|tiktok|app> [--select ID]... [--all] [--concurrency N] [--headed]
setup-2fa [--select EMAIL]... [--all] [--headless] [--limit N] [--concurrency N] [--rotate-proxy]
sms <balance|prices|number> [--country N] [--max-price X] [--yes]
whoami <x|app> --select CREDENTIAL [--json]
export <app> [--select EMAIL]...
get <app> --select ID...
set-status <app> <active|expired|restricted|escalated> --select ID...
list [--json]
```

IDs are emails for Google-backed accounts and usernames for X and TikTok. `--select` narrows any command to
the named IDs (repeatable); `--all` includes accounts that are already fine; `--limit N` caps the
run; `--rotate-proxy` uses a rotating proxy exit instead of the account's sticky one. `--concurrency N`
(default 1 = sequential) drives up to N accounts at once — it applies to every per-account command
(`login google|x|app`, `login <app> --by-email`, `verify`, `setup-2fa`). Each
account has its own profile, sticky exit and DB row, so the flows never collide, and DB and
credential-file writes are synchronous so parallel workers never clobber each other; a headed run
opens N browser windows and a held-open debug window holds its slot until closed, so with N accounts
and 100 to do, at most N run at a time.

The Google sign-in is one graph traversal, not a set of flag-gated modes. A single run walks the
whole sign-in graph — password, TOTP, and the phone step (always driven with a rented HeroSMS
number) — handling whatever challenge appears, in any order, without restarting. The reCAPTCHA is
met by clicking the real widget's checkbox (it often passes outright). When Google shows the image
grid instead, a headless run has a vision model (`codex exec`, `GRID_MODEL`) name the tiles holding
the requested object (bus, crosswalk, …) and clicks them and Verify, handling both the static
"select all squares" and the dynamic "verify once none left" grids, 3x3 and 4x4; the same model reads
the password-page text CAPTCHA off its image. A headed run never uses the vision model: it shows the
browser, and when the flow hits a node only a person clears (the image grid, the password-page text
CAPTCHA, or a real phone number once HeroSMS is exhausted) it leaves the page where Google put it,
waits, and resumes the moment the URL moves on. A headless run the model could not clear has no
window, so such a node escalates instead. There is no `--assist` and no `--sms`: "is a human here" is
just whether the run is headed, and the phone step is always attempted automatically. `--headed`
also, on any error, leaves the window open so you can inspect and finish by hand — close it to let
the run end.

Run long browser commands with the host's shell execution tool and retain the process/session
handle, stdout, stderr and final exit status. Wait for completion through the host's supported
background task or session mechanism. No stdin or terminal code entry is needed: people enter
verification codes directly in the headed browser. Report `ASSIST NEEDED` messages to the user.
For diagnosis, inspect saved `debug/<id>/<step>-<ts>/screenshot.png` and `info.txt`; a saved
capture shows the page at capture time, not a live desktop view. Do not call host-specific
`request_access` or `switch_display` tools. Page captures use Playwright in both headed and
headless modes; headed window recordings use the bundled Swift ScreenCaptureKit recorder.

### import

Reads `<state>/google/*.txt`, `<state>/x/*.txt` or `<state>/tiktok/*.txt` (or the named files), one account per line,
and upserts each into its table. Google lines are `email:password:totp_secret:app_password`
(last two optional; `app_password` is the 16-char Google app password used only for IMAP). X
lines come from the vendor: the first four fields are `username:password:email:email_password`,
the TOTP secret is the base32 field among the rest, a 40-hex `auth_token` is kept when present.
TikTok lines are `username:password:email:email_password:profile_url`; the first four fields are
kept (the email pair is optional).
Rows are keyed by email (google) / username (x, tiktok): re-running is a no-op, an edited file
re-imports, and a later file never nulls an email/TOTP an earlier one set. An X line that brings
another `auth_token` for a known account drops that row's ct0 and cookies (a ct0 only works with
its own session's token), so intel’s `fetch-x-mentions/scripts/verify-x.mjs` derives a fresh pair. `#` comments and blank
lines are skipped; an undecodable X line is reported and skipped, a bad Google line aborts with
its file and line number.

### login

- **google** — sign every selected account into Google (password + TOTP, and the phone step via
  HeroSMS) in its own persistent Camoufox profile. `restricted` accounts are skipped; `escalated`
  accounts ARE retried here (headed by default, so a human can clear a reCAPTCHA), and a successful sign-in clears the status back to
  `active`. (For `login <app>`, an `escalated` account is still skipped until cleared.) An address
  Google cannot find ("Couldn't find this account") and a verify-it's-you chooser with no
  authenticator path (it offers only a recovery email, another device, …) mark it `restricted`. Google's
  optional post-login setup wizard (`gds.google.com/web/*`: add a recovery phone, set a home
  address, …) is skipped automatically by going straight to the dashboard — no clicking through its
  cards, and no dependence on their localized button text. A profile that is already signed in
  lands on the dashboard from the sign-in page itself and nothing more is done.
- **adapter apps** — sign into Google when needed, then drive the adapter's Google OAuth
  flow and store its scoped cookies and localStorage. Default: missing or expired sessions;
  `--all` refreshes every selected account. An account whose app session is `restricted` is
  never retried; an adapter's `bannedResponse` hook records that verdict when the app refuses a
  banned account at sign-in.
- **x** — password login through a real Camoufox browser (X blocks the headless onboarding API):
  identifier → password → TOTP, then `auth_token`+`ct0`+cookie jar are stored, status `active`.
  Default: rows without a `ct0`. An unresolved challenge marks the row `escalated`.
- **tiktok** — username + password login through a real Camoufox browser on one ISP pool slot (a
  fixed IP, the pool of `~/.config/secrets-manager/.env`; not the residential proxy): the
  account's stored slot, else the pool's last one. The tiktok.com cookies and the slot are stored,
  status `active`; a session is only to be used from that slot (`--rotate-proxy` does not apply).
  Default: rows with no session or an `expired` one. A profile that still holds a session is
  checked against tiktok.com first: really signed in, it is stored as is; ended on TikTok's side,
  it signs in again. A login button still disabled after the form is filled triggers one retry
  with a fresh browser profile, keeping the old profile as a backup; disabled again, the row is
  marked `escalated`. An account TikTok reports as banned/suspended at login is marked `restricted`.
  A new device gets "Verify it's really you" after the password: the script
  picks the Email method. In a `--headed` run, read the code from the account’s mailbox and
  enter it directly in the TikTok browser window; the script submits a complete 6-digit code.
  No code within 5 minutes marks the row `escalated`. A headless run encountering email
  verification marks it `escalated` and requests a rerun with `--headed`; there is no terminal
  input. The mailbox itself is never opened by the script.
- **app --by-email** — dispatch to the adapter's `byEmail({db, cred, opts, io})` hook.
  A missing hook fails explicitly. The adapter owns alias login/signup and optional password minting.

### setup-2fa

Provisions the second factor for Google accounts that arrived as `email:password` only, turning
them into the full `email:password:totp_secret:app_password` shape the rest of the store expects.
For each selected account, in its own already-signed-in profile: adds a Google Authenticator app
and scrapes its base32 TOTP secret, turns on 2-Step Verification (skipping the "add a phone
number" prompt so it stays authenticator-only), and mints a Gmail app password — writing the TOTP
secret and app password back to the credential file (the app password is never printed). Requires
a live Google session per account; a logged-out account is reported with "run `login google`
first" and left as-is (not `escalated`). The three steps are independent, so a run resumes an
account from wherever it stopped; by default an account that already has both a TOTP secret and an
app password is skipped, `--all` redoes every step. A Google challenge the script cannot pass
marks the account `escalated`. Watch the first account in its window — the browser flow is tuned live,
not unit-tested, and the enrollment dialog's selectors drift.

### sms

Rents temporary phone numbers from HeroSMS to receive Google's verification SMS, so a login that
hits Google's phone step does not need a number handed in by a person. HeroSMS speaks the
SMS-Activate protocol; the key is `HERO_SMS_API_KEY` in `~/.config/secrets-manager/.env`. Three verbs:

- **balance** — the account balance (`getBalance`), free.
- **prices** — the countries offering a Google (`go`) number in stock at or under `--max-price`
  (default $0.05), cheapest first, with the numeric country id and stock (`getPrices`), free.
  Use it to learn a country's id (they are HeroSMS's own numeric ids, not dial codes).
- **number** — rent one number, wait up to 5 minutes for the code, print it and close the
  activation (`setStatus 6`); on no code, cancel for a refund (`setStatus 8`). Picks the cheapest
  affordable country unless `--country N` is given. **Spends money**, so it is a dry run (prints the
  quote) unless `--yes` is passed, and `--max-price` is clamped to $0.10. HeroSMS auto-refunds a
  number that receives no SMS within 20 minutes, so a wasted rent costs nothing.

Some countries never deliver Google's SMS (Indonesia has not); prefer a country proven to work.
The API is called from the home IP (it is the user's own SMS account, not a Google surface), not
through the residential proxy.

### verify

Checks that each selected account is still usable and persists the result. By default only
`active` accounts are checked; `--all` checks every one. `google`, `x` and `tiktok` are builtin
checks; any other target calls the adapter's `verify({db, email, session, opts, io})` hook, which
returns `active`, `restricted` or `expired`. A check that cannot tell (network error, 429, a stale
queryId, an unexpected page) throws: the status is left unchanged and the command exits 1.
An `expired` result never overwrites `restricted` or `escalated` — a banned or person-blocked
account's token is usually dead too — and is logged as `expired (kept restricted)`; any other
result is written.

- **google** — opens the account's own profile on its sticky residential exit and reads where
  myaccount.google.com leaves it: the dashboard or setup wizard is `active`, Google's marketing or
  sign-in page `expired`, the disabled speedbump `restricted`, another sign-in challenge
  `escalated`. Plus-alias rows (`base+tag@`) are app alias accounts, not Google sign-ins, and are skipped.
- **x** — one GraphQL `Viewer` call with the stored `auth_token` + `ct0` through the residential
  proxy (no browser, no `x-client-transaction-id`). The handle it names matching the row is
  `active`; auth errors 32/89/215 `expired`; 64 (suspended) `restricted`; 326 (locked, a person
  must unlock it) `escalated`; a token signing in as another handle throws. Needs `X_BEARER` and
  `X_VIEWER_QUERY_ID`; a 404 means the queryId is stale — re-read it from the `main.<hash>.js` that
  x.com/home serves a signed-in account (`queryId:"…",operationName:"Viewer"`).
- **tiktok** — opens the account's profile on its own ISP slot and reads the handle tiktok.com's
  explore page names: the account is `active`, no one `expired`, TikTok's ban text `restricted`.

Neither browser check ever signs in. Deriving a ct0 for a vendor X `auth_token` stays in the
intel plugin's `fetch-x-mentions/scripts/verify-x.mjs`.

### whoami

`whoami <x|app> --select CREDENTIAL [--json]` identifies the user behind one external
credential. Here `--select` is the credential itself, not a stored account ID; its type
is defined by the app adapter. It requires exactly one
nonempty value and accepts no account-selection or browser flags.
The adapter's optional `whoami({credential})` hook returns `{email: string}`.
No database is opened and no local account or status is written. The account need not
be imported or logged in locally. Stdout is the email, or `{"app":"<app>","email":"..."}`
with `--json`. Unsupported adapters, invalid credentials or malformed identities exit 1
with an error on stderr; credentials are redacted from errors.

`whoami x --select AUTH_TOKEN [--json]` is builtin. It accepts one 40-hex X
`auth_token`, automatically obtains `ct0`, then queries GraphQL Viewer through the
residential proxy. No local account, stored ct0 or browser is needed. Stdout is
`@username`, or `{"app":"x","username":"..."}` with `--json`; it does not promise
an email. Requires `X_BEARER`, `X_VIEWER_QUERY_ID` and `RESIDENTIAL_PROXY_URL`.
Expired, restricted, locked or inconclusive sessions fail without changing local state.

### export

`export <app> [--select EMAIL]...` calls `adapter.credentials(session)` for each selected
account with an active app session. The hook returns `Record<string, string> | null`.
Each object prints one JSON line to stdout: `{"app":"<app>","email":"<email>",...fields}`.
Credential values are printed in clear; callers decide how to use them. A null result prints
`<email>\tmissing` to stderr. Stdout contains only JSON lines, so it pipes cleanly into jq.
There is no file-output option or environment-variable naming in the adapter interface.
An adapter without the hook fails with `<app> has no credentials hook`.

### get / set-status / list

`get` prints the stored session of each `--select`ed account; `set-status` sets their status (both need `--select`, repeatable; a missing account is reported and makes the exit 1); `list` shows every Google account and
its app statuses. Existing app tables remain readable even without their adapters.

## State

`SECRETS_MANAGER_STATE_PATH` (default `~/.config/secrets-manager`), outside the repo, plaintext:

- `secrets.sqlite` — tables `google` (accounts), `x` (accounts + tokens), `tiktok` (accounts +
  cookies + ISP slot), and one per app
  (cookies, localStorage, status per account). Status everywhere:
  `active | expired | restricted | escalated` (restricted = banned by the app, escalated = needs a
  human). `scripts/schema.sql` has the fixed tables; app tables are created on first save.
- `google/*.txt`, `x/*.txt`, `tiktok/*.txt` — credential files.
- `profiles/<id>/` — one persistent Camoufox profile per Google email / X username / TikTok username / plus-alias
  (the "device" the site sees).
- `debug/<id>/<step>-<ts>/` — screenshot + `info.txt` of a failed step.

## Config

`~/.config/secrets-manager/.env`, loaded automatically without overriding the environment:

```
RESIDENTIAL_PROXY_URL=http://user:pass@host:port
HERO_SMS_API_KEY=...        # optional — rents phone numbers for the SMS step
ISP_PROXY_URL=http://user:pass@host:port  # TikTok fixed ISP pool
ISP_PROXY_COUNT=1                      # TikTok slots
X_BEARER=...                 # verify x: x.com's web-app bearer token
X_VIEWER_QUERY_ID=...        # verify x: the GraphQL Viewer queryId from x.com's main.js
```

Compatible proxies that take `sessid`/`sesstime` in the username give each account a sticky
exit (`sessid` derived from its ID) so one login never hops IPs. The browser runs with `geoip` so timezone/locale/WebRTC match the exit. Without a proxy,
browser commands refuse to run. Before every browser opens, its exit is checked (a request to
`www.gstatic.com/generate_204`, two tries): an exit that cannot tunnel fails that account with
`proxy exit down: <host> …` and leaves its status unchanged — sticky exits do go down for a while
(CONNECT answers 522); re-run later. `--rotate-proxy` changes the IP on every connection, so it is
no cure for a browser login.

To keep proxy traffic down, every browser context aborts requests a login never needs before
they leave the browser (`scripts/traffic.mjs`): all images, media and fonts on any site, plus a
blackhole list of the apps' market-data/telemetry/asset hosts and adapter-provided WebSockets. Add
hosts there (wildcards allowed). Blocked requests never touch the proxy; nothing is ever sent
from the home IP.

## Target apps

External ES modules default-export `(kit) => Adapter[]`. Apps are defined outside the plugin;
the generic engine completes Google OAuth and scopes exports to each adapter domain.
Adapter names are the table names, so existing session tables remain usable.

The adapter kit provides `clickFirst(page, texts, timeout = 15000, misses = [])`: it returns
a boolean and appends `{text, reason}` for each click timeout to the optional `misses` array.
Reasons retain the trimmed Playwright message/call log, capped at 2000 characters.
`debug.capture(page, email, label, note)` optionally saves that text as `screenshot.txt`
beside the capture screenshot; capture remains best-effort and never throws.

## Tests

```sh
SKILL_DIR="/absolute/path/to/loaded/secrets-manager"
node --test "$SKILL_DIR/tests/"*.mjs
```

Offline tests cover the store, credential parsing, TOTP, config, restrictions, OTP extraction,
page helpers, adapters, CLI dispatch and setup reporting. Live login selectors need headed verification.

## Limits

- reCAPTCHA grids and the password-page text CAPTCHA are auto-solved by the vision model on headless
  runs only (`--headless`); it is not 100%, and its misses get accounts banned, so Google sign-ins
  default to headed. A headless miss marks the account `escalated`; re-run headed to clear it by hand.
- The phone step (`challenge/iap`) is always driven with a rented HeroSMS number — the flow handles
  it with no flag. That path is unproven: Google has so far refused to send its verification SMS to
  every rented number (Cameroon, Canada, a real Philippines number all failed on fresh accounts with
  a residential US exit), so a fresh, low-trust account rejects the number regardless of format or
  country. The Google-side entry is correct (national digits only, dial code in the picker — the full
  `+code` in the field makes Google build a malformed number); the open question is a number source
  Google will accept. When every preferred country is exhausted, a headed run hands the window over
  so a person types a real number and takes the SMS on their own phone — the reliable path today;
  headless, it escalates. Rented numbers auto-refund in 20 min if no code arrives, so the automatic
  attempts cost ~nothing.
- A Cloudflare CAPTCHA is never solved by the script; run headed and click it yourself.
- Selectors drift; tune `login.mjs` / your adapter against a live account.
