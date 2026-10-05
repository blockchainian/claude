# creator

Run creator accounts on TikTok. The accounts, their credentials, browser profiles and status
live in the secrets plugin's `secrets-manager` store; this plugin only reads it.

## Skills

- **`upload-tiktok-video`** — post one mp4 to the store's earliest imported `active` TikTok
  account through TikTok Studio's upload page, in the account's own Camoufox profile on its ISP
  slot (the same device and IP it logged in with). Sets the caption, who can see it and the
  AI-generated label, then finds the posted video's id. The browser window is shown and
  screen-recorded for debugging. Each post is appended to `posts.jsonl`.
- **`fetch-tiktok-stats`** — sample the plays, likes, comments, shares and saves of the
  account's videos, read anonymously through TikTok's own signed web API, appended to
  `stats.jsonl` as a time series.

Account health and status (logged out, restricted, banned) belong to `secrets-manager`.

## Setup

```sh
cd "${CLAUDE_PLUGIN_ROOT}/skills/upload-tiktok-video/scripts" && npm install
```

Both skills use that install. Camoufox itself is fetched by the secrets plugin's setup.
`ISP_PROXY_URL` is read from the secrets-manager's `.env`. Data goes to
`~/.local/share/creator/tiktok/` (`CREATOR_TIKTOK_DIR` overrides it). Recording uses
ScreenCaptureKit through `swift` and needs the Screen Recording permission.

## Tests

```sh
npm run test:creator
```
