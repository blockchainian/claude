---
name: secrets-manager
description: Manage local plaintext credentials and browser sessions — import Google/X/TikTok accounts, log in through external app adapters, check that accounts are still usable (verify), prepare app accounts (setup), provision Google 2FA, rent SMS numbers, and export app credentials. Use when asked to get (搞) or set up a new account, log an account into Google, X, TikTok or an app, check whether accounts still work, turn on 2FA, or hand an app's credentials to another tool.
---

# Secrets manager

Run the CLI from this skill, with `SKILL_DIR` set in every shell call to the absolute directory
containing this loaded `SKILL.md`; `node "$SKILL_DIR/scripts/cli.mjs" --help` lists every command and
flag. Claude Code and Codex share the same accounts, so close a profile's browser before opening that
account from the other host. To write an app adapter, read
[references/adapters.md](references/adapters.md).

## Setup (after every plugin install or update)

```sh
SKILL_DIR="/absolute/path/to/loaded/secrets-manager"
"$SKILL_DIR/scripts/setup.sh"
# Read-only report:
"$SKILL_DIR/scripts/setup.sh" --check
```

Fill in `~/.config/secrets-manager/.env` from the plugin's `.env.example` (variables below) and list
the app adapters in `~/.config/secrets-manager/config.json`, checking each file with
`node "$SKILL_DIR/scripts/cli.mjs" validate /absolute/path/adapters.mjs`:

```json
{"adapters": ["/absolute/path/adapters.mjs"]}
```

A vendor whose Google lines carry the TOTP seed in a URL needs that URL's pattern in the same file,
as `"totpUrlPatterns": ["<regex whose first group captures the base32 seed>"]`.

Headed runs need Accessibility trust to move OAuth popups onto the `BROWSER_DISPLAY` display and
Screen Recording permission to record the windows.

## Environment variables

Shell values take precedence over `~/.config/secrets-manager/.env`.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `RESIDENTIAL_PROXY_URL` | Residential login proxy | For related feature | ~/.config/secrets-manager/.env |
| `ISP_PROXY_URL` | Fixed ISP proxy pool | For related feature | ~/.config/secrets-manager/.env |
| `ISP_PROXY_COUNT` | Pool slot count; default 1 | Optional | ~/.config/secrets-manager/.env |
| `HERO_SMS_API_KEY` | HeroSMS key for phone rentals | For related feature | ~/.config/secrets-manager/.env |
| `X_BEARER_TOKEN` | X web-client bearer token | For related feature | ~/.config/secrets-manager/.env |
| `X_VIEWER_QUERY_ID` | X Viewer operation ID | For related feature | ~/.config/secrets-manager/.env |
| `BROWSER_DISPLAY` | Headed browser display: any part of its name, any case (e.g. `SAMSUNG`); unset means the main display | Optional | ~/.config/secrets-manager/.env |
| `CAPTCHA_RESOLVER_MODEL` | Captcha vision resolver model; default gpt-6-sol | Optional | ~/.config/secrets-manager/.env |
| `CAPTCHA_RESOLVER_MODEL_EFFORT` | Resolver reasoning effort; default low | Optional | ~/.config/secrets-manager/.env |
| `CAPTCHA_RESOLVER_MODEL_TIER` | Resolver service tier; default fast | Optional | ~/.config/secrets-manager/.env |
| `SECRETS_STATE_DIR` | Accounts, credential files and logged-in browser profiles; default ~/.local/state/secrets-manager | Optional | ~/.config/secrets-manager/.env |
| `SECRETS_DATA_DIR` | Debug captures and scratch files; default ~/.local/share/secrets-manager | Optional | ~/.config/secrets-manager/.env |

## New account flow

A request to "搞" (get) a new account means this whole sequence for a fresh Google account, every
app your adapters define included. Run each step on its own:

1. `node "$SKILL_DIR/scripts/cli.mjs" import google <file>`.
2. `node "$SKILL_DIR/scripts/cli.mjs" login google --select <email>`.
3. `node "$SKILL_DIR/scripts/cli.mjs" login <app> --select <email>` for each required adapter.
4. `node "$SKILL_DIR/scripts/cli.mjs" setup <app> --select <email>` for adapters with a setup hook.

The account is done when Google is `active` and each required app session is `ready` (or `active`
when its adapter has no setup hook); report each app's status from `list`.

Google sign-ins (`login google`, `login <app>`, `setup-2fa`) run headed so a person can clear any
CAPTCHA; keep them headed, because the headless vision solver's misses get accounts banned. In a
headed run a person also clears whatever the script cannot and enters verification codes in the
window, and after an error the window stays open until it is closed. Don't rerun a failed Google
sign-in over and over: repeated re-auth locks the account out for hours.

A failed step leaves a status that says what to do. `escalated` means a person must clear a
challenge: rerun that step headed. `restricted` means the app banned the account: stop for that app.
`expired` means the session lapsed: rerun the step. An app step needs Google `active`, so fix
`login google` before chasing an app failure. Take a fresh batch one account at a time with
`--select`, and widen to `--all` or `--concurrency N` once the flow works on a few; for `setup`, set
`--concurrency` from the app's measured rate limit.

## Commands

- **import** reads the credential files under `SECRETS_STATE_DIR` (`google/`, `x/`, `tiktok/`), or the
  named files, one account per line, and reports any line it cannot parse. Reruns are safe. Import only
  burner accounts into X, never the user's own: intel's X fetchers read with every `active` X row.
  After importing a new `auth_token` for a known X account, derive its ct0 with intel's
  `fetch-x-mentions/scripts/verify-x.mjs`.
- **login tiktok** binds the session to the account's fixed ISP slot, so use it only from that slot.
  On a new device TikTok emails a code: in a `--headed` run, read it from the account's mailbox and
  type it into the TikTok window within 5 minutes.
- **setup-2fa** completes a password-only Google account with an authenticator TOTP, 2-Step
  Verification and an app password. Run `login google` first, and watch the first account in its
  window, since the enrollment dialog's selectors drift.
- **sms** rents HeroSMS numbers for Google's phone step. `number` spends money, so it only quotes
  until you pass `--yes`; `prices` is free and gives the country ids `--country` takes. Never pass
  `--country` for Cameroon, Indonesia, the Philippines or Kenya: they do not deliver Google's code,
  and `--country` bypasses `SMS_COUNTRY_BLACKLIST` in `scripts/config.mjs`, which keeps them out
  otherwise.
- **verify** checks that accounts still work and saves the result; exit 1 means a check could not
  tell, and nothing was changed. A 404 from `verify x` means `X_VIEWER_QUERY_ID` is stale: re-read it
  from the `main.<hash>.js` that x.com/home serves a signed-in account
  (`queryId:"…",operationName:"Viewer"`).
- **setup** runs the adapter's setup hook on `active` app sessions, and on `ready` ones too with
  `--all`; `setup <app> --help` lists the app's own flags.
- **export** prints app credentials as JSON lines with the values in clear.

## Running and diagnosing

Run long browser commands in the background and wait for their exit status, and pass every
`ASSIST NEEDED` message on to the user. Do not call host-specific `request_access` or
`switch_display` tools. To diagnose a failure, read the step's `screenshot.png` and `info.txt` under
`SECRETS_DATA_DIR/debug/<id>/<step>-<ts>/`, which show the page at capture time rather than the live
desktop; a headless Google sign-in also leaves a video of every page under
`SECRETS_DATA_DIR/debug/<id>/rec-<ts>/`.

`proxy exit down: <host> …` means the account's sticky exit is down for a while (CONNECT answers
522): rerun later. `--rotate-proxy` changes the IP on every connection, so it cannot rescue a browser
login.

## Limits

- Google often refuses to send its SMS to rented numbers on fresh accounts. A headed run then hands
  the window to a person, who types a real number and takes the SMS on their own phone.
- A Cloudflare CAPTCHA is never solved by the script; run headed and click it yourself.
- Selectors drift, and the offline tests (`node --test "$SKILL_DIR/tests/"*.mjs`) do not cover them:
  tune `login.mjs` or your adapter against a live account, headed.
