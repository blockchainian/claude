# intel

Gather and distill knowledge from **long-form sources** — articles, podcasts,
talks, videos, live streams — into text you can read and search. The source
goes in; durable, attributable notes come out.

## Skills

- **`transcribe`** — turn audio into plain-text words, transcribed
  locally with whisper (`mlx-whisper`, Apple Silicon). No cloud, no API key.
  Handles a finite file or URL (batch) and an ongoing **live stream** — a
  direct HLS/Icecast/RTMP URL, or a Twitch/YouTube/X Spaces live resolved with
  `streamlink`/`yt-dlp` — segmented and transcribed as it plays. Idempotent
  `setup.sh` auto-installs what's missing. A reusable audio-to-text step: any
  skill that has audio and needs its words calls this one.
- **`digest`** — turn a source URL into durable highlights, stored and
  searchable. Extracts an article's main body (trafilatura) or a podcast
  transcript, reads YouTube via subtitles, and for a page that only offers
  audio falls back to `transcribe`. A bare URL digests; `save` / `search`
  manage the store, with per-item take-aways.

## Why they live together

Both skills answer the same question — *what was actually said or written, and
what of it is worth keeping* — from different source shapes. `transcribe`
is the floor: it gets words out of sound. `digest` builds highlighting and a
searchable store on top, over whatever produced the words. New source kinds
(Twitch VODs, conference talks) slot in by reusing `transcribe` for the
audio leg and `digest` for the notes.

## Requirements

- `transcribe`: Apple Silicon; `setup.sh` installs `ffmpeg`,
  `streamlink`, `yt-dlp`, and a whisper runner (`mlx_whisper`/`uv`) via
  Homebrew. `curl` for URL downloads.
- `digest`: `curl`; `setup.sh` installs `uv` (runs the trafilatura article
  extractor) and `yt-dlp` (YouTube subtitles).

## Tests

```
python3 skills/transcribe/scripts/test_transcribe.py
python3 skills/digest/scripts/test_digest.py
```
