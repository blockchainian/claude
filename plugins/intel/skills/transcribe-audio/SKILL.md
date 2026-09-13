---
name: transcribe-audio
description: >
  Turn audio into plain-text words, transcribed locally with whisper — a
  finite file or URL, or a live stream captured as it plays. Use when you have
  recorded or ongoing audio (a podcast episode, an interview, a Twitch or
  YouTube live, an X Space, a radio stream) and need its transcript, and the
  source offers no text of its own. Other skills call this as their
  audio-to-text step.

  NOT for: reading a text transcript that already exists (fetch it directly),
  or video where you only need the audio track (extract the audio first).
---

# Transcribe audio — local whisper, file, URL, or live stream

One whisper engine (`whisper-large-v3-turbo`, Apple Silicon), two entry points:
a **batch** script for finite audio, a **live** script for an ongoing stream.
Both write plain text and print JSON about it.

## Setup (automatic, idempotent)

Run setup once at the start; it installs only what is missing and is a near-instant
no-op when everything is present, so it is safe to run every time.

```bash
bash "${CLAUDE_PLUGIN_ROOT}/skills/transcribe-audio/scripts/setup.sh"
```

It ensures `ffmpeg` (pulls and segments audio), `streamlink` and `yt-dlp`
(resolve platform lives), and a whisper runner (`mlx_whisper`, or `uv` to run it
in an ephemeral env) — installing the missing ones with Homebrew. The whisper
model (~1.5GB) is not fetched here: mlx-whisper downloads it on the first
transcription and caches it, so it self-installs once. `setup.sh --check` reports
what is present or missing without installing anything.

## Batch — a finite file or URL

```bash
python3 "${CLAUDE_PLUGIN_ROOT}/skills/transcribe-audio/scripts/transcribe_audio.py" \
  "<audio-url-or-file>" "<out.txt>"
```

- A local path is transcribed in place; a URL is downloaded first (`curl`,
  10-minute cap).
- Prints JSON with `transcript` (the out path), `words`, `thin` (`words < 1500`)
  and `source`.

## Live — an ongoing stream

```bash
python3 "${CLAUDE_PLUGIN_ROOT}/skills/transcribe-audio/scripts/transcribe_live.py" \
  "<stream>" "<out.txt>" [--segment-seconds 30] [--max-minutes N]
```

- `<stream>` is a direct, ffmpeg-readable URL (HLS/`.m3u8`, Icecast/radio, RTMP,
  http) **or** a platform live — Twitch and YouTube are resolved with
  `streamlink`/`yt-dlp`, X Spaces attempted with `yt-dlp`.
- `ffmpeg` segments the stream into `--segment-seconds` chunks (30s default,
  matching whisper's window); each **completed** chunk is transcribed and
  **appended** to `<out.txt>`, so the file is tail-able while the run continues.
  Per-chunk text is also echoed to stderr.
- It stops when the stream ends, at `--max-minutes` if given, or on Ctrl-C
  (SIGINT) — in every case it lets the open chunk finalize and transcribes it
  before exiting, then prints a JSON summary (`words`, `chunks`, ...).
- It is long-running by nature: run it in the **background** and tail `<out.txt>`.

## What the transcript is, and is not

- **No speaker labels.** Whisper emits a single stream of text, so attribute
  quotes to the source, not to a named speaker.
- **Whatever the audio actually carries.** Downloaded episode audio often has
  **dynamically-inserted modern ads** that are not part of the original
  recording; whisper can also **loop**, repeating a line over a garbled or
  musical stretch. Note both and exclude or repair them when you read.
- **Verbatim, not edited.** It keeps filler and false starts; compress when you
  summarise, but do not claim the transcript says something it does not.

## Requirements

Apple Silicon. `setup.sh` installs the rest (Homebrew required). The first run
without `setup.sh` errors and names the missing tool.

## Tests

`scripts/test_transcribe.py` covers the batch and live command shapes, the
chunk-readiness logic, platform resolution, and `setup.sh --check`; when `ffmpeg`
and a whisper runner are present it also runs a real end-to-end batch and live
transcription of a generated clip:

```bash
python3 "${CLAUDE_PLUGIN_ROOT}/skills/transcribe-audio/scripts/test_transcribe.py"
```
