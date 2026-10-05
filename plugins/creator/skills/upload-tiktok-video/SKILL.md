---
name: upload-tiktok-video
description: Post one mp4 to the TikTok account the secrets-manager skill logged in — its earliest imported active account — through TikTok Studio's upload page in that account's own Camoufox profile on its ISP slot, with a caption, who can see it, the AI-generated label, the promotion disclosure and a commercial-library sound; --headed shows and screen-records the browser to debug a broken step; the posted video's id is found and the post logged to posts.jsonl. Use when asked to upload / post / 上传 / 发 a video to TikTok, or to test the upload flow. NOT for logging the account in or its status (secrets-manager), NOT for the video's numbers afterwards (fetch-tiktok-stats) and NOT for the official Content Posting API.
---

# Upload a TikTok video

```
node "${CLAUDE_PLUGIN_ROOT}/skills/upload-tiktok-video/scripts/upload-tiktok-video.mjs" <video.mp4> \
  [--username <name>] [--caption <text>] [--visibility everyone|friends|only-me] [--aigc] \
  [--promotion your-brand|branded-content|your-brand,branded-content] [--sound <id>] \
  [--headed [--with-sound]]
```

- `--username`: post as that account of the store; it must be `active`. Default below.
- `--caption`: the description, hashtags included (`"my words #tag #other"`). Default empty.
- `--visibility`: who can see the post. Default `everyone`.
- `--aigc`: turn on TikTok's "AI-generated content" label (under "Show more").
- `--promotion`: turn on "Disclose post content" (under "Show more") and tick `your-brand` (the
  account promotes itself), `branded-content` (a paid partnership with another brand; posting it
  agrees to TikTok's Branded Content Policy), or both, comma-separated.
- `--sound`: add the commercial-library sound with that id (fetch-tiktok-sounds lists them). It
  plays under the video's own audio, and the post links to it as its sound.
- `--headed`: show the browser window and screen-record it. Use it when a step fails: TikTok
  Studio's page changes, and the selectors with it. Without it the browser is hidden.
- `--with-sound`: with `--headed`, unmute the browser (Playwright mutes it) to hear the video.

Launch it in the background and reread the log; a post takes from about 20 s to a few minutes,
most of it waiting for the video to show on the profile.

## The account

`--username`, else the secrets-manager store's earliest imported (`created_at`) `active` TikTok
account with an ISP slot. It is opened exactly as `login tiktok` opened it: the same profile directory
(`<state>/profiles/<username>`), the same stored fingerprint, the same ISP slot, English locale.
The store is only read. A profile that is no longer signed in stops the run with
`run the secrets-manager skill's login tiktok`; log it in there and run again. Nothing else may
have the profile open at the same time (a fetch-tiktok-mentions run using the same account, a
`login tiktok`).

## What it does

1. Opens `tiktokstudio/upload`; with `--headed`, shows the window and starts recording it.
2. Chooses the file and waits for TikTok's "Uploaded".
3. Whenever an overlay gets in the way of a step (never waiting for one otherwise): closes the
   "new editing features" tour, and accepts the offer to turn on automatic content checks (music
   copyright and For You eligibility, an account setting). Both show only on a profile's first
   few visits.
4. With `--sound`: reads the sound's title by its id, opens the editor's Sounds panel, searches the
   title, presses "+" on the result with that id (titles repeat) and saves the edit.
5. Replaces the prefilled description (the file name) with the caption, typed key by key.
6. Sets who can see the post, the AI-generated label and the promotion disclosure when asked
   (each turns on at once, no confirmation).
7. Presses Post, confirming "Post now" if TikTok asks, and waits to leave the upload page.
8. Opens the account's profile and finds the new video by its create time, through the same
   page-signed API calls fetch-tiktok-stats uses, every 5 s, moved randomly by up to 1.5 s, for up to five minutes.

## Output

Under `~/.local/share/creator/tiktok/<username>/`, one directory per account (`CREATOR_TIKTOK_DIR`
overrides `~/.local/share/creator/tiktok`):

- `posts.jsonl`: one line per post,
  `{ at, username, file, caption, visibility, aigc, promotion, sound, videoId, url, recording }`.
  `videoId` is null when the video had not reached the profile in time.
- `recordings/upload-<ts>.mov`: the screen recording of a `--headed` run, also on failure. When a
  step fails, rerun with `--headed` and watch it first.

## Setup

`npm install` in this skill's `scripts/` (fetch-tiktok-stats uses the same install). Camoufox is
fetched by the secrets plugin's setup. `ISP_PROXY_URL` comes from the secrets-manager's `.env`.
Recording needs macOS's Screen Recording permission for the terminal running `swift`.

## Not here

Scheduling and the Content Posting API.
