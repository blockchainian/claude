---
name: fetch-tiktok-stats
description: Record the current plays, likes, comments, shares and saves of every video on a TikTok account's profile — by default the secrets-manager account upload-tiktok-video posts as — read anonymously through TikTok's own signed web API in Camoufox, one row per video per run appended to a time series. Use when asked to check / 看 / 监测 how the uploaded TikTok videos are doing, or to take a stats sample for the time series. NOT for account health or status (secrets-manager owns that), NOT for uploading (upload-tiktok-video) and NOT for other people's videos (intel's fetch-tiktok-mentions).
---

# Fetch TikTok stats

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
node "$SKILL_DIR/scripts/fetch-tiktok-stats.mjs" [--username <name>] [--headed [--with-sound]]
```

- `--username` reads another account; default the account upload-tiktok-video posts as: the
  secrets-manager store's earliest imported `active` TikTok account.
- `--headed` shows the browser window and screen-records it to
  `<username>/recordings/stats-<ts>.mov`, to debug a read that stopped working; `--with-sound`
  also unmutes it.
- One run is one sample. Run it again later (a schedule, or by hand) to build the series.

Each run appends one row per video to `~/.local/share/creator/tiktok/<username>/stats.jsonl`
(`CREATOR_TIKTOK_DIR` overrides `~/.local/share/creator/tiktok`) and prints them:

```
{ at, username, videoId, createTime, playCount, diggCount, commentCount, shareCount, collectCount }
```

## How it works

TikTok's web API answers an unsigned request with an empty 200, and the TikTok-Api library is not
used. A fresh anonymous Camoufox browser on the account's ISP slot opens tiktok.com, copies the
query params of the first API request the page sends, and calls `user/detail/` (the account's
secUid) and `post/item_list/` with `fetch()` inside the page, where TikTok's own script signs them.
This is the same method intel's fetch-tiktok-mentions uses. No login is needed: the account's
profile is not opened, and the store is only read.

An anonymous viewer gets the first page of a profile, about 35 videos, newest first; older videos
are not sampled. A video posted a minute ago may not show to an anonymous viewer yet (measured: 0
videos about 20 s after posting, the video a minute later); it is in the next sample.

Needs the upload-tiktok-video skill's `npm ci` (the browser code lives there) and
`ISP_PROXY_URL` in the secrets-manager's `.env`.
