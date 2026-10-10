---
name: fetch-tiktok-stats
description: Record the current plays, likes, comments, shares and saves of every video on a TikTok account's profile — by default the secrets-manager account upload-tiktok-video posts as — read anonymously through TikTok's own signed web API in Camoufox, one row per video per run appended to a time series. Use when asked to check / 看 / 监测 how the uploaded TikTok videos are doing, or to take a stats sample for the time series. NOT for account health or status (secrets-manager owns that), NOT for uploading (upload-tiktok-video) and NOT for other people's videos (intel's fetch-tiktok-mentions).
---

# Fetch TikTok stats

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. The
script imports the sibling `upload-tiktok-video` skill's `scripts/tiktok-session.mjs` runtime and
uses its dependencies, so keep the whole creator plugin installed.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Environment variables

Copy the Creator plugin’s `.env.example` to `~/.config/creator/.env`; Claude Code, Codex and all worktrees use this file. Existing shell values take precedence.

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `ISP_PROXY_URL` | ISP pool matching the account’s login pool | Yes | ~/.config/creator/.env |
| `BROWSER_DISPLAY` | Display for headed browser windows | Optional | ~/.config/creator/.env |
| `CREATOR_STATE_DIR` | State root; default ~/.local/state/creator, with each account's posts.jsonl and stats.jsonl under tiktok/<username>/ | Optional | ~/.config/creator/.env |
| `CREATOR_DATA_DIR` | Data root; default ~/.local/share/creator, with each account's screen recordings under tiktok/<username>/recordings/ | Optional | ~/.config/creator/.env |
| `SECRETS_STATE_DIR` | Existing Secrets Manager account/profile directory | Optional | ~/.config/creator/.env |

Account credentials, sessions and profiles remain in Secrets Manager; Creator configuration is separate.

## Setup

The script needs Node.js 22.13+ with `node:sqlite` and the Camoufox browser that secrets-manager
installs. Install the runtime's npm dependencies once for the installed creator plugin; all four
creator skills share this install:

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
(cd "$SKILL_DIR/../upload-tiktok-video/scripts" && npm ci)
```

Creator only reads the existing secrets-manager store, which Claude Code and Codex share at
`~/.local/state/secrets-manager` unless `SECRETS_STATE_DIR` points elsewhere; it holds the TikTok
account rows, ISP slots and browser profiles. Installing creator on another host does not create
or migrate accounts, so when the account is missing, provision or log it in with secrets-manager
(`login tiktok`).

## Run

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
node "$SKILL_DIR/scripts/fetch-tiktok-stats.mjs" [--username <name>] [--headed [--with-sound]]
```

By default the script reads the account upload-tiktok-video posts as, which is the store's
earliest imported `active` TikTok account; `--username` reads another account instead. To debug a
read that stopped working, `--headed` shows the browser window and screen-records it to
`<username>/recordings/stats-<ts>.mov` under the data root, and `--with-sound` also unmutes it.

One run is one sample. The script appends one row per video to
`tiktok/<username>/stats.jsonl` under the state root and prints the counts; run it again later, on
a schedule or by hand, to build the series. Each row has this shape:

```
{ at, username, videoId, createTime, playCount, diggCount, commentCount, shareCount, collectCount }
```

## How the read works

TikTok's web API answers an unsigned request with an empty 200, so the script does not use the
TikTok-Api library. It opens tiktok.com in a fresh anonymous Camoufox browser on the account's ISP
slot, copies the query params of the first API request the page sends, and calls `user/detail/`
(for the account's secUid) and `post/item_list/` with `fetch()` inside the page, where TikTok's own
script signs them. This is the same method as intel's fetch-tiktok-mentions. No login is needed:
the account's profile is never opened.

## Limits

An anonymous viewer gets only the first page of a profile, about 35 videos, newest first, so older
videos are not sampled. A video posted a minute ago may not show to an anonymous viewer yet: in one
measurement it was missing about 20 seconds after posting and present a minute later. It appears in
the next sample.
