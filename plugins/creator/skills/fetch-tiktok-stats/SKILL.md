---
name: fetch-tiktok-stats
description: Record the current plays, likes, comments, shares and saves of every video on a TikTok account's profile — by default the secrets-manager account upload-tiktok-video posts as — read anonymously through TikTok's own signed web API in Camoufox, one row per video per run appended to a time series. Use when asked to check / 看 / 监测 how the uploaded TikTok videos are doing, or to take a stats sample for the time series. NOT for account health or status (secrets-manager owns that), NOT for uploading (upload-tiktok-video) and NOT for other people's videos (intel's fetch-tiktok-mentions).
---

# Fetch TikTok stats

```
node "${CLAUDE_PLUGIN_ROOT}/skills/fetch-tiktok-stats/scripts/fetch-tiktok-stats.mjs" [--username <name>]
```

- `--username` reads another account; default the account upload-tiktok-video posts as: the
  secrets-manager store's earliest imported `active` TikTok account.
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

Needs the upload-tiktok-video skill's `npm install` (the browser code lives there) and
`ISP_PROXY_URL` in the secrets-manager's `.env`.
