---
name: open-tiktok-account
description: Open a secrets-manager TikTok account's own Camoufox profile in a shown browser window, signed in as that account on its ISP slot, at TikTok Studio or any tiktok.com page, so a person can look around — private (only-me) posts, Studio analytics, settings — optionally with sound; the run ends when the window is closed. Use when asked to open / 打开 a TikTok account, or to see or hear a post only the account can see. NOT for posting (upload-tiktok-video), NOT for numbers (fetch-tiktok-stats) and NOT for logging the account in (secrets-manager).
---

# Open TikTok account

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. Keep the
whole creator plugin installed, because all four of its skills run on the runtime and dependencies of
`upload-tiktok-video/scripts/tiktok-session.mjs`.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Environment variables

Copy the creator plugin's `.env.example` to `~/.config/creator/.env`, the one file every host and
worktree reads. Values already set in the shell take precedence.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `ISP_PROXY_URL` | ISP pool matching the account's login pool | Yes | `~/.config/creator/.env` |
| `BROWSER_DISPLAY` | Display the browser window opens on | Optional | `~/.config/creator/.env` |
| `SECRETS_STATE_DIR` | Existing secrets-manager store of accounts, ISP slots and profiles; default `~/.local/state/secrets-manager` | Optional | `~/.config/creator/.env` |

## Setup

The script needs Node.js 22.13+ with `node:sqlite` and the Camoufox browser that secrets-manager
installs. Install the runtime's npm dependencies once for the plugin; all four skills share this
install:

```sh
(cd "$SKILL_DIR/../upload-tiktok-video/scripts" && npm ci)
```

Account credentials, sessions and profiles stay in the secrets-manager store, which Claude Code and
Codex share; creator only reads it. Installing creator on another host creates and migrates no
accounts, so provision or log in a missing account with secrets-manager.

## Run

```sh
node "$SKILL_DIR/scripts/open-tiktok-account.mjs" [--username <name>] [--url <url>] [--with-sound]
```

- `--username` picks the store's account, in any status: a `restricted` one opens too, to see TikTok's
  ban notice or appeal. The default is the account upload-tiktok-video posts as.
- `--url` is the `https://www.tiktok.com/` page to open. The default is TikTok Studio's home.
- `--with-sound` unmutes the browser, which Playwright otherwise mutes, so the videos can be heard.

The window is always shown, so there is no `--headed`, and nothing is recorded. It opens the profile
`login tiktok` signed in, as that same device and IP. The run holds until the person closes the
window, so start it in the background and wait for it to exit; never let a timeout close the
person's browser.

If the log says the profile is logged out, or the script finds no such account, run the
secrets-manager skill's `login tiktok` for it. Nothing else may have the profile open at the same
time, so have the window closed before uploading or listing sounds as that account.
