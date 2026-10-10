---
name: secrets-manager
description: Manage local plaintext credentials and browser sessions — import Google/X/TikTok accounts, log in through external app adapters, check that accounts are still usable (verify), prepare app accounts (setup), provision Google 2FA, rent SMS numbers, and export app credentials. Use when asked to get (搞) or set up a new account, log an account into Google, X, TikTok or an app, check whether accounts still work, turn on 2FA, or hand an app's credentials to another tool.
---

# Secrets manager

The CLI and account state are the same in Claude Code and Codex; installing in another host does
not migrate accounts. Close a profile's browser before opening the same account from the other host.

Run the CLI from this skill, with `SKILL_DIR` set in every shell call to the absolute directory
containing this loaded `SKILL.md`. `node "$SKILL_DIR/scripts/cli.mjs" --help` lists every command
and flag. Browser commands need the residential proxy in `~/.config/secrets-manager/.env` and refuse
to run without it. To write an app adapter, read [references/adapters.md](references/adapters.md).

## Setup (after every plugin install or update)

```sh
SKILL_DIR="/absolute/path/to/loaded/secrets-manager"
"$SKILL_DIR/scripts/setup.sh"
# Read-only report:
"$SKILL_DIR/scripts/setup.sh" --check
```

App adapters are listed in `~/.config/secrets-manager/config.json`; without it no app adapters load
and every other command still works:

```json
{"adapters": ["/absolute/path/adapters.mjs"]}
```

Check an adapter file with `node "$SKILL_DIR/scripts/cli.mjs" validate /absolute/path/adapters.mjs`.
Headed runs open on the display `BROWSER_DISPLAY` names (any part of its name, any case, e.g.
`SAMSUNG`; unset means the main display); moving OAuth popups there needs Accessibility trust, and
recording the windows needs Screen Recording permission.

## New account flow

A request to "搞" (get) a new account means this whole sequence for a fresh Google account, every
app your adapters define included. Run each step on its own:

1. `node "$SKILL_DIR/scripts/cli.mjs" import google <file>`.
2. `node "$SKILL_DIR/scripts/cli.mjs" login google --select <email>`.
3. `node "$SKILL_DIR/scripts/cli.mjs" login <app> --select <email>` for each required adapter.
4. `node "$SKILL_DIR/scripts/cli.mjs" setup <app> --select <email>` for adapters with a setup hook.

Done means Google is `active` and each required app session is `ready` when its adapter has a setup
hook, otherwise `active`. `list` shows the statuses; report each app's.

Every Google sign-in (`login google`, `login <app>`, `setup-2fa`) runs headed unless `--headless` is
passed, so a person at the window clears any CAPTCHA: the headless vision solver's misses get
accounts banned. `login x|tiktok`, `verify` and `setup` run headless unless `--headed`. With
`--headed`, a step only a person can clear waits in the window until they do, and on any error the
window stays open to inspect; close it to end the run. Don't rerun a failed Google sign-in over and
over: repeated re-auth locks the account out for hours.

A failed step records a status that says what to do next:

- `escalated` — a challenge the script could not pass. Rerun that step headed so a person can clear
  it; an app step skips the account until `login google` succeeds.
- `restricted` — banned. Stop for that app; the session is unusable.
- `expired` — the session lapsed. Rerun that step.

A failed step does not stop the others, but an app step needs Google `active`, so fix `login google`
before chasing an app failure. Take a fresh batch one account at a time with `--select`, watching
each outcome, and widen to `--all` or `--concurrency N` once the flow works on a few. Set
`--concurrency` for `setup` from the app's measured rate limit.

## Running and diagnosing

Run long browser commands in the background with the host's shell tool and wait for the exit status.
Nothing is read from stdin: a person enters verification codes in the headed browser. Report
`ASSIST NEEDED` messages to the user. To diagnose a failure, read the saved
`debug/<id>/<step>-<ts>/screenshot.png` and `info.txt` under `SECRETS_DATA_DIR`; they show the page
at capture time, not the live desktop. Headless Google sign-ins also save a video of every page under
`debug/<email>/rec-<ts>/`. Do not call host-specific `request_access` or `switch_display` tools.

A browser command that fails with `proxy exit down: <host> …` hit a sticky exit that is down for a
while (CONNECT answers 522): rerun later. `--rotate-proxy` changes the IP on every connection, so it
is no cure for a browser login.

## Commands

- **import** reads `<state>/google/*.txt`, `<state>/x/*.txt` or `<state>/tiktok/*.txt`, or the named
  files, one account per line. Google lines are `email:password:totp_secret:app_password` (the last
  two optional); X lines are the vendor's, starting `username:password:email:email_password`; TikTok
  lines are `username:password:email:email_password:profile_url`. Reruns are safe. Import only burner
  accounts into X: intel's X fetchers read with every `active` X row, unfiltered, so never import the
  user's own accounts. After importing a
  new `auth_token` for a known X account, derive its ct0 with intel's
  `fetch-x-mentions/scripts/verify-x.mjs`.
- **login tiktok** uses the account's fixed ISP slot, and its session works only from that slot. A new
  device gets "Verify it's really you" by email: in a `--headed` run, read the code from the account's
  mailbox and type it into the TikTok window within 5 minutes; a headless run escalates instead.
- **setup-2fa** turns `email:password`-only Google accounts into full ones: it enrolls an
  authenticator TOTP, turns on 2-Step Verification and mints an app password, writing both to the
  credential file. Run `login google` first. Watch the first account in its window: the enrollment
  dialog's selectors drift.
- **sms** rents HeroSMS numbers for Google's phone step (`HERO_SMS_API_KEY`). `balance` and `prices`
  are free; `prices` gives each country's HeroSMS id for `--country`. `number` **spends money**: it
  only prints the quote unless `--yes` is passed. A number that gets no code is refunded. Prefer a
  country proven to deliver Google's SMS; some never do.
- **verify** checks that accounts are still usable and saves the result; a check that cannot tell
  exits 1 and changes nothing. A 404 from `verify x` means `X_VIEWER_QUERY_ID` is stale: re-read it
  from the `main.<hash>.js` that x.com/home serves a signed-in account
  (`queryId:"…",operationName:"Viewer"`).
- **setup** runs the adapter's setup hook on accounts whose app session is `active` (`--all` adds
  `ready` ones for a rerun) and marks them `ready`. `setup <app> --help` lists the app's own flags.
- **whoami** names the user behind one credential passed as `--select`, without touching stored
  accounts; `whoami x` takes a 40-hex `auth_token`.
- **export** prints each `active` or `ready` app session's credentials as JSON lines, values in clear.
- **get**, **set-status** and **list** read a stored session, set statuses by hand, and show every
  account's statuses.

## Environment Variables

Copy the plugin's `.env.example` to `~/.config/secrets-manager/.env`; shell values take precedence.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `RESIDENTIAL_PROXY_URL` | Residential login proxy | For related feature | ~/.config/secrets-manager/.env |
| `ISP_PROXY_URL` | Fixed ISP proxy pool | For related feature | ~/.config/secrets-manager/.env |
| `ISP_PROXY_COUNT` | Pool slot count; default 1 | Optional | ~/.config/secrets-manager/.env |
| `HERO_SMS_API_KEY` | HeroSMS key for phone rentals | For related feature | ~/.config/secrets-manager/.env |
| `X_BEARER_TOKEN` | X web-client bearer token | For related feature | ~/.config/secrets-manager/.env |
| `X_VIEWER_QUERY_ID` | X Viewer operation ID | For related feature | ~/.config/secrets-manager/.env |
| `BROWSER_DISPLAY` | Headed browser display | Optional | ~/.config/secrets-manager/.env |
| `CAPTCHA_RESOLVER_MODEL` | Captcha vision resolver model; default gpt-6-sol | Optional | ~/.config/secrets-manager/.env |
| `CAPTCHA_RESOLVER_MODEL_EFFORT` | Resolver reasoning effort; default low | Optional | ~/.config/secrets-manager/.env |
| `CAPTCHA_RESOLVER_MODEL_TIER` | Resolver service tier; default fast | Optional | ~/.config/secrets-manager/.env |
| `SECRETS_STATE_DIR` | Accounts, credential files and logged-in browser profiles; default ~/.local/state/secrets-manager | Optional | ~/.config/secrets-manager/.env |
| `SECRETS_DATA_DIR` | Debug captures and scratch files; default ~/.local/share/secrets-manager | Optional | ~/.config/secrets-manager/.env |

## Tests

```sh
SKILL_DIR="/absolute/path/to/loaded/secrets-manager"
node --test "$SKILL_DIR/tests/"*.mjs
```

The tests are offline; live login selectors need headed verification.

## Limits

- Google often refuses to send its SMS to rented numbers on fresh accounts. A headed run then hands
  the window to a person, who types a real number and takes the SMS on their own phone; headless, the
  step escalates.
- A Cloudflare CAPTCHA is never solved by the script; run headed and click it yourself.
- Selectors drift; tune `login.mjs` or your adapter against a live account.
