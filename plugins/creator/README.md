# creator

Run creator accounts on TikTok from Claude Code or Codex. See
[installation](../../README.md#codex). The accounts, their
credentials, browser profiles and status live in the secrets plugin's `secrets-manager` store; this plugin only reads it.

## Environment variables

Copy the Creator plugin’s `.env.example` to `~/.config/creator/.env`; both hosts and all worktrees use this file. Existing shell values take precedence.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `ISP_PROXY_URL` | ISP pool matching the account’s login pool | Yes | ~/.config/creator/.env |
| `BROWSER_DISPLAY` | Display for headed browser windows | Optional | ~/.config/creator/.env |
| `CREATOR_STATE_DIR` | State root; default ~/.local/state/creator, with each account's posts.jsonl and stats.jsonl under tiktok/<username>/ | Optional | ~/.config/creator/.env |
| `CREATOR_DATA_DIR` | Data root; default ~/.local/share/creator, with each account's screen recordings under tiktok/<username>/recordings/ | Optional | ~/.config/creator/.env |
| `SECRETS_STATE_DIR` | Existing Secrets Manager account/profile directory | Optional | ~/.config/creator/.env |

Account credentials, sessions and profiles remain in Secrets Manager; Creator configuration is separate.

## Skills

- **`upload-tiktok-video`** — post one mp4 to the store's earliest imported `active` TikTok
  account through TikTok Studio's upload page, in the account's own Camoufox profile on its ISP
  slot (the same device and IP it logged in with). Sets the caption, who can see it, the
  AI-generated label, the promotion disclosure and a commercial-library sound, then finds the
  posted video's id. Each post is appended to `posts.jsonl`.
- **`fetch-tiktok-stats`** — sample the plays, likes, comments, shares and saves of the
  account's videos, read anonymously through TikTok's own signed web API, appended to
  `stats.jsonl` as a time series.
- **`fetch-tiktok-sounds`** — list the hottest sounds of TikTok's commercial (royalty-free)
  music library with the ids `upload-tiktok-video --sound` takes.
- **`open-tiktok-account`** — open the account's own browser profile in a shown window, signed
  in, for a person to look around (private posts, Studio analytics); `--with-sound` unmutes it.

Every skill but `open-tiktok-account` runs its browser hidden. `--headed` shows it and screen-records it to the account's
`recordings/`, for debugging a flow that TikTok's page changes broke; `--with-sound` also unmutes
it. A shown window goes on the display `BROWSER_DISPLAY` names in `~/.config/creator/.env`
(any part of its name, e.g. `SAMSUNG`; unset means the main display), as secrets-manager's headed
logins do; moving it needs the Accessibility permission for the terminal running `swift`.

Account health and status (logged out, restricted, banned) belong to `secrets-manager`.

## Setup

```sh
# Use the actual directory of the loaded upload-tiktok-video/SKILL.md.
SKILL_DIR="/absolute/path/to/loaded/skill"
(cd "$SKILL_DIR/scripts" && npm ci)
```

Requires Node.js 22.13+ with `node:sqlite`. Keep the full creator plugin installed:
all four skills import the upload skill's runtime by sibling-relative paths and use that install.
The browser flows run in Camoufox scripts without Claude Workflow, Claude API, or MCP browser
tools. Camoufox itself is fetched by the secrets plugin's setup.
Both hosts read the same existing secrets-manager state, by default `~/.local/state/secrets-manager`;
set `SECRETS_STATE_DIR` explicitly if yours is elsewhere. The store, logged-in profiles,
ISP slots must already be provisioned through secrets-manager. Creator’s ISP pool is configured separately at `~/.config/creator/.env` and must match the login pool.
Creator does not install secrets-manager or create/migrate its accounts; use the existing
secrets-manager setup when those prerequisites are missing. Never open the same account profile
in both hosts at once. The post log and stats go to
`~/.local/state/creator/tiktok/<username>/` and screen recordings to
`~/.local/share/creator/tiktok/<username>/recordings/`, one directory per account
(`CREATOR_STATE_DIR` and `CREATOR_DATA_DIR` override the roots). Recording uses
ScreenCaptureKit through `swift` and needs the Screen Recording permission.

Use the host's shell execution tool for the scripts; retain the process/session handle and
wait for completion instead of repeatedly polling logs. `open-tiktok-account` remains running
until the person closes its shown browser.

## Tests

```sh
npm run test:creator
```
