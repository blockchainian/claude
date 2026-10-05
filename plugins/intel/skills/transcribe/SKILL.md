---
name: transcribe
description: >
  Turn audio into plain-text words, transcribed locally with whisper — a
  finite file or URL, or a live stream captured as it plays. Use when you have
  recorded or ongoing audio (a podcast episode, an interview, a Twitch or
  YouTube live, an X Space, a radio stream) and need its transcript, and the
  source offers no text of its own. It also transcribes the audio track of a
  **video** file or stream directly — ffmpeg extracts the audio, so no manual
  step is needed. Other skills call this as their audio-to-text step.

  NOT for: reading a text transcript that already exists (fetch it directly),
  or anything in a video's picture (it transcribes the audio only — no slide
  OCR or scene description).
---

# Transcribe audio — local whisper, file, URL, or live stream

## Runtime and paths

Works in Claude Code and Codex. Resolve `SKILL_DIR` from the absolute directory of
this loaded `SKILL.md`, not the working directory or a host-specific environment variable:

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

Repeat this assignment and any `S`, `T` or `U` assignments used below in every shell call;
shell variables may not persist between calls. If the loaded path is unavailable, stop
and report it. Keep the full intel plugin installed: sibling skills share scripts.
Run archive commands from the repository that owns the archive; configuration and
account stores are shared between hosts and are not migrated by installing intel.

For finite long-running commands, choose a deadline before launch and retain the process
handle and output. In Claude Code use `run_in_background` and its completion notification;
in Codex use the shell tool's process/session handle and wait for completion. Subagents
must await their own commands before returning. Do not repeatedly poll logs or assume a
background completion wakes either host. On timeout, preserve diagnostics and report the
process state before retrying. Use the current host's image/file tools to inspect artifacts.

## Environment Variables

No skill-specific environment variables or `.env` file are required.

One whisper engine (`whisper-large-v3-turbo`, Apple Silicon), two entry points:
a **batch** script for finite audio, a **live** script for an ongoing stream.
Both write plain text and print JSON about it.

## Setup (automatic, idempotent)

Run setup once at the start; it installs only what is missing and is a near-instant
no-op when everything is present, so it is safe to run every time.

```bash
"$SKILL_DIR/scripts/setup.sh"
```

It ensures `ffmpeg` (pulls and segments audio), `streamlink` and `yt-dlp`
(resolve platform lives), and a whisper runner (`mlx_whisper`, or `uv` to run it
in an ephemeral env) — installing the missing ones with Homebrew. The whisper
model (~1.5GB) is not fetched here: mlx-whisper downloads it on the first
transcription and caches it, so it self-installs once. `setup.sh --check` reports
what is present or missing without installing anything.

## Batch — a finite file or URL

```bash
"$SKILL_DIR/scripts/transcribe-audio.mjs" \
  "<audio-url-or-file>" "<out.txt>"
```

- A local path is transcribed in place; a URL is downloaded first (`curl`,
  10-minute cap). The file may be **audio or video** — whisper decodes it with
  ffmpeg and transcribes the audio track either way.
- Prints JSON with `transcript` (the out path), `words`, `thin` (`words < 1500`)
  and `source`.

## Live — an ongoing stream

```bash
"$SKILL_DIR/scripts/transcribe-live.mjs" \
  "<stream>" "<out.txt>" [--segment-seconds 30] [--max-minutes N]
```

- `<stream>` is a direct, ffmpeg-readable URL (HLS/`.m3u8`, Icecast/radio, RTMP,
  http) **or** a platform live — Twitch and YouTube are resolved with
  `streamlink`/`yt-dlp`, X Spaces attempted with `yt-dlp`. A **video** stream is
  fine — ffmpeg takes the audio track and ignores the picture.
- `ffmpeg` segments the stream into `--segment-seconds` chunks (30s default,
  matching whisper's window); each **completed** chunk is transcribed and
  **appended** to `<out.txt>`, so the file is tail-able while the run continues.
  Per-chunk text is also echoed to stderr.
- It stops when the stream ends, at `--max-minutes` if given, or on Ctrl-C
  (SIGINT) — in every case it lets the open chunk finalize and transcribes it
  before exiting, then prints a JSON summary (`words`, `chunks`, ...).
- It is long-running by nature: choose `--max-minutes` before launch unless the user
  requested an ongoing stream. Retain the process handle; read `<out.txt>` when needed
  for partial results. Await the bounded run, or send SIGINT to stop and finalize an
  ongoing stream; do not use a blocking `tail -f` to wait for completion.

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

Apple Silicon and Node.js 18.18+. `setup.sh` installs the rest (Homebrew required). The first run
without `setup.sh` errors and names the missing tool.
