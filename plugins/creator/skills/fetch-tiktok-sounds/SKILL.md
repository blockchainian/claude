---
name: fetch-tiktok-sounds
description: List the hottest sounds of TikTok's commercial (royalty-free) music library — the list TikTok Studio's "Royalty-free sounds" page shows — with the ids upload-tiktok-video's --sound takes, read in the secrets-manager account's own signed-in Camoufox profile. Use when asked which music / 配乐 / 商用音乐 / 热歌 to put on a TikTok post, or for a sound id to upload with. NOT for adding the sound to a post (upload-tiktok-video --sound), NOT for video stats (fetch-tiktok-stats).
---

# Fetch TikTok sounds

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

## Runtime and paths

Works in Claude Code and Codex. Set `SKILL_DIR` to the absolute directory containing the
`SKILL.md` that the host loaded for this skill; replace the example value below with that actual
path. Do not use the project's working directory or a host-specific plugin-root variable.
Keep the full creator plugin installed: all four skills use its
`upload-tiktok-video/scripts/tiktok-session.mjs` runtime and dependencies.

Requires Node.js 22.13+ with `node:sqlite`, the upload-tiktok-video runtime's npm dependencies,
and the Camoufox browser installed by secrets-manager. The existing secrets-manager store is
shared across both hosts at `~/.local/state/secrets-manager` (`SECRETS_STATE_DIR` explicitly
overrides it), including TikTok account rows, ISP slots and browser profiles.
Configure `ISP_PROXY_URL` in `~/.config/creator/.env` or the process environment. Creator reads this
store; use secrets-manager to provision or log in an account if it is missing. Installing
creator in another host does not create or migrate accounts.

## Shared runtime setup

Run once for the installed creator plugin (all four skills use this same install):

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
(cd "$SKILL_DIR/../upload-tiktok-video/scripts" && npm ci)
```

## Run

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
node "$SKILL_DIR/scripts/fetch-tiktok-sounds.mjs" [--username <name>] [--count <n>] [--headed [--with-sound]]
```

- `--username`: read the list as that account of the store; default the account
  upload-tiktok-video posts as.
- `--count`: how many sounds, hottest first. Default 20.
- `--headed`: show the browser window and screen-record it to
  `<username>/recordings/sounds-<ts>.mov`, to debug a read that stopped working.
- `--with-sound`: with `--headed`, unmute the browser (Playwright mutes it), to play the sounds on
  the page.

Prints one line per sound:

```
7603363008859047972  Comedy Corridor — Finley Reed  1:49  1522060 posts
```

The first column is the id to pass to `upload-tiktok-video --sound`. "posts" is how many posts use
the sound.

## How it works

The list is Studio's own (`creator/music/unlimited/list`, about 770,000 sounds, sorted by hot), so
it needs a signed-in account: the account's Camoufox profile is opened on its ISP slot at
`tiktokstudio/sound-library`, and the list is fetched inside that page, 20 sounds a page. Nothing
else may have the profile open at the same time. Every sound on this list is cleared for use in
any post on TikTok, promotional ones included; the clearance does not extend to other platforms.

Needs the upload-tiktok-video skill's `npm ci` and `ISP_PROXY_URL` in the secrets-manager's
`.env`.
