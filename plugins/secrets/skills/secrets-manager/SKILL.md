---
name: secrets-manager
description: Manage local plaintext credentials and browser sessions, import Google/X/TikTok accounts, log in through external app adapters, verify app sessions, provision Google 2FA, and export refresh tokens.
---

# Secrets manager

Run from the repo root. All state lives under `SECRETS_MANAGER_STATE_PATH` (default
`~/.config/secrets-manager`); the residential proxy from `~/.config/secrets-manager/.env` is required for every
browser command (the home IP is never used).

## Setup (after every plugin install or update)

```sh
"${CLAUDE_PLUGIN_ROOT}/skills/secrets-manager/scripts/setup.sh"
# Read-only report:
"${CLAUDE_PLUGIN_ROOT}/skills/secrets-manager/scripts/setup.sh" --check
```

Setup installs dependencies, downloads Camoufox and writes `~/.local/bin/secrets-manager`.
Put `~/.local/bin` on PATH; the launcher points at this install's CLI.
Configure external adapters in `~/.config/secrets-manager/config.json`:

```json
{"adapters": ["/absolute/path/adapters.mjs"]}
```

`SECRETS_MANAGER_ADAPTERS` (colon-separated absolute paths) overrides that file.
Run `secrets-manager validate /absolute/path/adapters.mjs` to check the real kit contract.
Without a config file or override, no app adapters are loaded; other commands still work.
See the plugin README's Adapter interface for every field and kit helper.

Pinned: Camoufox v152.0.4-beta.30, camoufox-js 0.12.0, playwright-core 1.60.0. Headed runs open the window on the
built-in display (`builtinDisplay.swift`), move OAuth popups there (`moveWindows.swift`, needs
Accessibility trust) and record them (`recordWindows.swift`, needs Screen Recording
permission), both through `swift`.

## New account flow

A fresh Google account signs into the apps your adapters define. Run each step separately:

1. `secrets-manager import google <file>`.
2. `secrets-manager login google --only <email> --headed`.
3. `secrets-manager login <app> --only <email> --headed` for each required adapter.

Done means Google and every required app session are active; `list` shows their status.

Every step reuses the account's one profile, so an app step never re-does Google. When a step
fails it records a status that says how to proceed:

- `escalated` — a Google challenge the script could not pass. Re-run that step `--headed` so a
  person can clear it; a later successful `login google` clears the escalation.
- `restricted` — the app banned the account. Stop for that app; the session is unusable.
- `expired` — the session lapsed. Re-run that step.

A failed step does not halt the others, but an app step needs Google `active` first (an
`escalated` account is skipped by `login <app>`), so fix `login google` before chasing an app
failure. Run one account at a time (`--only`, `--headed`) for a fresh batch so each outcome is
watched; widen to `--all` / higher `--concurrency` once the flow is proven on a few.

## Commands

```
node "${CLAUDE_PLUGIN_ROOT}/skills/secrets-manager/scripts/cli.mjs" <command> [options]

import <google|x|tiktok> [file...]
login <google|x|tiktok|app> [--only ID]... [--all] [--limit N] [--concurrency N] [--headed] [--rotate-proxy]
login <app> --by-email [--mint-app-password] [--only EMAIL]... [--headed]
verify <app> [--only ID]... [--all] [--concurrency N] [--headed]
setup-2fa [--only EMAIL]... [--all] [--headed] [--limit N] [--concurrency N] [--rotate-proxy]
sms <balance|prices|number> [--country N] [--max-price X] [--yes]
export-env <app> [--only EMAIL]... [--out FILE]
get <app> <id>
set-status <app> <id> <active|expired|restricted|escalated>
list [--json]
```

IDs are emails for Google-backed accounts and usernames for X and TikTok. `--only` narrows any command to
the named IDs (repeatable); `--all` includes accounts that are already fine; `--limit N` caps the
run; `--rotate-proxy` uses a rotating proxy exit instead of the account's sticky one. `--concurrency N`
(default 1 = sequential) drives up to N accounts at once — it applies to every per-account command
(`login google|x|app`, `login <app> --by-email`, `verify app`, `setup-2fa`). Each
account has its own profile, sticky exit and DB row, so the flows never collide, and DB and
credential-file writes are synchronous so parallel workers never clobber each other; a headed run
opens N browser windows and a held-open debug window holds its slot until closed, so with N accounts
and 100 to do, at most N run at a time.

The Google sign-in is one graph traversal, not a set of flag-gated modes. A single run walks the
whole sign-in graph — password, TOTP, and the phone step (always driven with a rented HeroSMS
number) — handling whatever challenge appears, in any order, without restarting. The reCAPTCHA and
the password-page text CAPTCHA are cleared automatically when `CAPSOLVER_API_KEY` is set in
`~/.config/secrets-manager/.env`. The reCAPTCHA is solved by driving the real widget — click the checkbox (it often
passes outright), and when Google shows the image grid, CapSolver classifies which tiles hold the
requested object (bus, crosswalk, …) and the script clicks them and Verify, handling both the static
"select all squares" and the dynamic "verify once none left" grids, 3x3 and 4x4. The text CAPTCHA is
read off its image. With no CapSolver key, `--headed` is the fallback:
it shows the browser, and when the flow hits a node only a person can clear (a reCAPTCHA the solver
did not clear, or a real phone number once HeroSMS is exhausted) it leaves the page where Google put
it, waits, and resumes the moment the URL moves on. A headless run with no solver has no window to
clear, so such a node escalates instead. There is no `--assist` and no `--sms`: "is a human here" is
just whether the run is headed, and the phone step is always attempted automatically. `--headed`
also, on any error, leaves the window open so you can inspect and finish by hand — close it to let
the run end.

When a headed run is backgrounded and the log stalls — an `>>> ASSIST NEEDED` line, or no
progress while the browser process is still alive — Claude MUST proactively look at the Camoufox
window with computer-use instead of idling on the log or waiting to be told: `request_access` for
Camoufox (bundleId `org.mozilla.camoufox`), `switch_display` to the display it moved to (the
built-in one), and `screenshot`. Browsers are read-tier, so Claude can SEE but not click; the point
is to name the real blocker and the exact control the user must click, and to catch a stuck node the
graph mishandles (e.g. a reCAPTCHA checkbox already passed where the true block is an unclicked
**Next**, which the handoff message may still mislabel "click the reCAPTCHA").

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
  accounts ARE retried here (run `--headed` so a human can clear a reCAPTCHA), and a successful sign-in clears the status back to
  `active`. (For `login <app>`, an `escalated` account is still skipped until cleared.) Google's
  optional post-login setup wizard (`gds.google.com/web/*`: add a recovery phone, set a home
  address, …) is skipped automatically by going straight to the dashboard — no clicking through its
  cards, and no dependence on their localized button text. A profile that is already signed in
  lands on the dashboard from the sign-in page itself and nothing more is done.
- **adapter apps** — sign into Google when needed, then drive the adapter's Google OAuth
  flow and store its scoped cookies and localStorage. Default: missing or expired sessions;
  `--all` refreshes every selected account.
- **x** — password login through a real Camoufox browser (X blocks the headless onboarding API):
  identifier → password → TOTP, then `auth_token`+`ct0`+cookie jar are stored, status `active`.
  Default: rows without a `ct0`. An unresolved challenge marks the row `escalated`.
- **tiktok** — username + password login through a real Camoufox browser on one ISP pool slot (a
  fixed IP, the pool of `~/.config/secrets-manager/.env`; not the residential proxy): the
  account's stored slot, else the pool's last one. The tiktok.com cookies and the slot are stored,
  status `active`; a session is only to be used from that slot (`--rotate-proxy` does not apply).
  Default: rows with no session or an `expired` one. A profile that still holds a session is
  checked against tiktok.com first: really signed in, it is stored as is; ended on TikTok's side,
  it signs in again. A login button still disabled after the form is filled marks the row
  `escalated`. An account TikTok reports as banned/suspended at login is marked `restricted`.
  A new device gets "Verify it's really you" after the password: the script
  picks the Email method and asks for the emailed 6-digit code on the terminal (its subject is
  "NNNNNN is your 6-digit code"), then types it in. The code can also be typed into the
  `--headed` window, which submits it once all 6 digits are in; not entered either way within
  5 minutes, the row is marked `escalated`. Log
  TikTok accounts in one at a time: the prompts of several would share one terminal. The mailbox itself is never opened by the script.
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
marks the account `escalated`. Run the first account `--headed` — the browser flow is tuned live,
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

`verify <app>` calls the adapter's `verify({db, email, session, opts, io})` hook and persists
its `active`, `restricted` or `expired` result. Errors leave the session unchanged and fail the command.
X token verification moved to the intel plugin's `fetch-x-mentions/scripts/verify-x.mjs`.

### export-env

`export-env <app>` calls `adapter.exportEnv.token(session)` on active sessions. Prints only
presence and counts; `--out FILE` writes `adapter.exportEnv.envVar=token1,token2`.
Missing hooks and unknown adapter targets fail with loaded names.

### get / set-status / list

`get` prints a stored session; `set-status` updates it; `list` shows every Google account and
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
CAPSOLVER_API_KEY=...        # optional — auto-solves the reCAPTCHA and password-page CAPTCHA
```

Compatible proxies that take `sessid`/`sesstime` in the username give each account a sticky
exit (`sessid` derived from its ID) so one login never hops IPs. The browser runs with `geoip` so timezone/locale/WebRTC match the exit. Without a proxy,
browser commands refuse to run. `CAPSOLVER_API_KEY` is optional: with it, the reCAPTCHA and the
password-page text CAPTCHA are cleared automatically; without it they fall back to the `--headed`
human click.

To keep proxy traffic down, every browser context aborts requests a login never needs before
they leave the browser (`scripts/traffic.mjs`): all images, media and fonts on any site, plus a
blackhole list of the apps' market-data/telemetry/asset hosts and adapter-provided WebSockets. Add
hosts there (wildcards allowed). Blocked requests never touch the proxy; nothing is ever sent
from the home IP.

## Target apps

External ES modules default-export `(kit) => Adapter[]`. Apps are defined outside the plugin;
the generic engine completes Google OAuth and scopes exports to each adapter domain.
Adapter names are the table names, so existing session tables remain usable.

## Tests

```sh
node --test "${CLAUDE_PLUGIN_ROOT}/skills/secrets-manager/tests/"*.mjs
```

Offline tests cover the store, credential parsing, TOTP, config, restrictions, OTP extraction,
page helpers, adapters, CLI dispatch and setup reporting. Live login selectors need headed verification.

## Limits

- reCAPTCHA and the password-page text CAPTCHA are auto-solved by CapSolver when `CAPSOLVER_API_KEY`
  is set; no solver is 100%, so a failed solve still falls back to the `--headed` human click. A
  headless run with no key (or a solver miss) that hits a CAPTCHA marks the account `escalated` and
  moves on; re-run `--headed`, or set the key, so it can be cleared.
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
