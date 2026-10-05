---
name: fetch-tiktok-sounds
description: List the hottest sounds of TikTok's commercial (royalty-free) music library — the list TikTok Studio's "Royalty-free sounds" page shows — with the ids upload-tiktok-video's --sound takes, read in the secrets-manager account's own signed-in Camoufox profile. Use when asked which music / 配乐 / 商用音乐 / 热歌 to put on a TikTok post, or for a sound id to upload with. NOT for adding the sound to a post (upload-tiktok-video --sound), NOT for video stats (fetch-tiktok-stats).
---

# Fetch TikTok sounds

```
node "${CLAUDE_PLUGIN_ROOT}/skills/fetch-tiktok-sounds/scripts/fetch-tiktok-sounds.mjs" [--username <name>] [--count <n>]
```

- `--username`: read the list as that account of the store; default the account
  upload-tiktok-video posts as.
- `--count`: how many sounds, hottest first. Default 20.

Prints one line per sound:

```
7603363008859047972  Comedy Corridor — Finley Reed  1:49  1522060 posts
```

The first column is the id to pass to `upload-tiktok-video --sound`. "posts" is how many posts use
the sound.

## How it works

The list is Studio's own (`creator/music/unlimited/list`, about 770,000 sounds, sorted by hot), so
it needs a signed-in account: the account's Camoufox profile is opened headless on its ISP slot at
`tiktokstudio/sound-library`, and the list is fetched inside that page, 20 sounds a page. Nothing
else may have the profile open at the same time. Every sound on this list is cleared for use in
any post on TikTok, promotional ones included; the clearance does not extend to other platforms.

Needs the upload-tiktok-video skill's `npm install` and `ISP_PROXY_URL` in the secrets-manager's
`.env`.
