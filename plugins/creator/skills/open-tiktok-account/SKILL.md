---
name: open-tiktok-account
description: Open a secrets-manager TikTok account's own Camoufox profile in a shown browser window, signed in as that account on its ISP slot, at TikTok Studio or any tiktok.com page, so a person can look around — private (only-me) posts, Studio analytics, settings — optionally with sound; the run ends when the window is closed. Use when asked to open / 打开 a TikTok account, or to see or hear a post only the account can see. NOT for posting (upload-tiktok-video), NOT for numbers (fetch-tiktok-stats) and NOT for logging the account in (secrets-manager).
---

# Open a TikTok account

## Runtime and paths

Works in Claude Code and Codex. Set `SKILL_DIR` to the absolute directory containing the
`SKILL.md` that the host loaded for this skill; replace the example value below with that actual
path. Do not use the project's working directory or a host-specific plugin-root variable.
Keep the full creator plugin installed: all four skills use its
`upload-tiktok-video/scripts/tiktok-session.mjs` runtime and dependencies.

Requires Node.js 22.13+ with `node:sqlite`, the upload-tiktok-video runtime's npm dependencies,
and the Camoufox browser installed by secrets-manager. The existing secrets-manager store is
shared across both hosts at `~/.config/secrets-manager` (`SECRETS_MANAGER_STATE_PATH` explicitly
overrides it), including its `.env`, TikTok account rows, ISP slots and browser profiles.
`ISP_PROXY_URL` must be configured there or in the process environment. Creator reads this
store; use secrets-manager to provision or log in an account if it is missing. Installing
creator in another host does not create or migrate accounts.

## Shared runtime setup

Run once for the installed creator plugin (all four skills use this same install):

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
npm ci --prefix "$SKILL_DIR/../upload-tiktok-video/scripts"
```

## Run

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
node "$SKILL_DIR/scripts/open-tiktok-account.mjs" \
  [--username <name>] [--url <url>] [--with-sound]
```

- `--username`: the account of the store, in any status: a `restricted` one opens too, to see
  TikTok's ban notice or appeal. Default the account upload-tiktok-video posts as.
- `--url`: the tiktok.com page to open. Default TikTok Studio's home.
- `--with-sound`: unmute the browser (Playwright mutes it), to hear the videos.

Run through the host's shell execution tool and retain its process/session handle and output.
It holds until the person closes the window, so use a background execution session. Wait on
its completion event when needed; do not repeatedly poll the process or log. A review deadline
must not silently close the person's browser. The window is always
shown, so there is no `--headed`, and nothing is recorded. The profile is the one `login tiktok`
signed in, opened as that same device and IP; a logged-out profile is said so in the log. Nothing
else may have the profile open at the same time, so close the window before uploading or listing
sounds as that account.
