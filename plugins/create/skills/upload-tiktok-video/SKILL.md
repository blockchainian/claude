---
name: upload-tiktok-video
description: Post one mp4 to the TikTok account the secrets-manager skill logged in — its earliest imported active account — through TikTok Studio's upload page in that account's own Camoufox profile on its ISP slot, with a caption, who can see it and the AI-generated label; the browser window is shown and screen-recorded; the posted video's id is found and the post logged to posts.jsonl. Use when asked to upload / post / 上传 / 发 a video to TikTok, or to test the upload flow. NOT for logging the account in or its status (secrets-manager), NOT for the video's numbers afterwards (track-tiktok-stats) and NOT for the official Content Posting API.
---

# Upload a TikTok video

```
node "${CLAUDE_PLUGIN_ROOT}/skills/upload-tiktok-video/scripts/upload-tiktok-video.mjs" <video.mp4> \
  [--caption <text>] [--visibility everyone|friends|only-me] [--ai-generated]
```

- `--caption`: the description, hashtags included (`"my words #tag #other"`). Default empty.
- `--visibility`: who can see the post. Default `everyone`.
- `--ai-generated`: turn on TikTok's "AI-generated content" label (under "Show more").

Launch it in the background and reread the log; a post takes a few minutes, most of it waiting for
the video to show on the profile.

## The account

The secrets-manager store's earliest imported (`created_at`) `active` TikTok account with an ISP
slot. It is opened exactly as `login tiktok` opened it: the same profile directory
(`<state>/profiles/<username>`), the same stored fingerprint, the same ISP slot, English locale.
The store is only read. A profile that is no longer signed in stops the run with
`run the secrets-manager skill's login tiktok`; log it in there and run again. Nothing else may
have the profile open at the same time (a fetch-tiktok-mentions run using the same account, a
`login tiktok`).

## What it does

1. Opens `tiktokstudio/upload` headed and starts recording the window.
2. Chooses the file and waits for TikTok's "Uploaded".
3. Closes the "new editing features" tour and declines the offer to turn on automatic content
   checks (account settings are left as they are).
4. Replaces the prefilled description (the file name) with the caption, typed key by key.
5. Sets who can see the post, and the AI-generated label when asked.
6. Presses Post, confirming "Post now" if TikTok asks, and waits to leave the upload page.
7. Opens the account's profile and finds the new video by its create time, through the same
   page-signed API calls track-tiktok-stats uses, for up to five minutes.

## Output

Under `~/.local/share/create/tiktok/` (`CREATE_TIKTOK_DIR` overrides it):

- `posts.jsonl`: one line per post,
  `{ at, username, file, caption, visibility, aiGenerated, videoId, url, recording }`.
  `videoId` is null when the video had not reached the profile in time.
- `recordings/<username>-<ts>.mov`: the screen recording of the run, also on failure. Watch it
  first when a step fails: TikTok Studio's page changes, and the selectors with it.

## Setup

`npm install` in this skill's `scripts/` (track-tiktok-stats uses the same install). Camoufox is
fetched by the secrets plugin's setup. `ISP_PROXY_URL` comes from the secrets-manager's `.env`.
Recording needs macOS's Screen Recording permission for the terminal running `swift`.

## Not here

Music from TikTok's commercial library, the "promotes a brand" disclosure, scheduling, and the
Content Posting API.
