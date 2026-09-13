# intel

Gather and distill knowledge from **spoken and broadcast media** — podcasts,
interviews, streams — into text you can read and search. Recorded talk goes in;
durable, attributable notes come out.

## Skills

- **`transcribe-audio`** — turn audio into plain-text words, transcribed
  locally with whisper (`mlx-whisper`, Apple Silicon). No cloud, no API key.
  Handles a finite file or URL (batch) and an ongoing **live stream** — a
  direct HLS/Icecast/RTMP URL, or a Twitch/YouTube/X Spaces live resolved with
  `streamlink`/`yt-dlp` — segmented and transcribed as it plays. Idempotent
  `setup.sh` auto-installs what's missing. A reusable audio-to-text step: any
  skill that has audio and needs its words calls this one.
- **`podcast`** — turn a podcast transcript URL into durable highlights, stored
  and searchable. Reads ordinary transcript pages and YouTube subtitles; for a
  thin page that only offers audio, it falls back to `transcribe-audio`.

## Why they live together

Both skills answer the same question — *what was actually said, and what of it
is worth keeping* — from different source shapes. `transcribe-audio` is the
floor: it gets words out of sound. `podcast` builds highlighting and a
searchable store on top. New source kinds (Twitch VODs, X Spaces, conference
talks) slot in beside `podcast`, reusing `transcribe-audio` for the audio leg.

## Requirements

- `transcribe-audio`: Apple Silicon; `setup.sh` installs `ffmpeg`,
  `streamlink`, `yt-dlp`, and a whisper runner (`mlx_whisper`/`uv`) via
  Homebrew. `curl` for URL downloads.
- `podcast`: `curl`; `yt-dlp` for YouTube sources.

## Tests

```
python3 skills/transcribe-audio/scripts/test_transcribe.py
```
