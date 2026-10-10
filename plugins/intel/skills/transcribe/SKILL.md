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

# Transcribe

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. The
scripts import from the sibling `fetch-x-mentions` skill, so keep the whole intel plugin installed.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Environment variables

| Variable | Purpose | Required | Set in |
|---|---|---|---|
| `INTEL_OUTPUT_DIR` | Output root; transcripts go under `transcripts/`; default `~/Documents` | Optional | `~/.config/intel/.env` |

## Setup

The skill needs Apple Silicon, Node.js 18.18+ and Homebrew. Run setup at the start of every use;
it installs only what is missing, with Homebrew, and returns at once when nothing is:

```bash
"$SKILL_DIR/scripts/setup.sh"
```

It ensures `ffmpeg`, `streamlink`, `yt-dlp` and a whisper runner (`mlx_whisper`, or `uv` to run
it in an ephemeral env). `setup.sh --check` only reports what is present or missing. The
whisper model, `whisper-large-v3-turbo` (about 1.5 GB), downloads on the first transcription and
is cached after that. Without setup, the first run fails and names the missing tool.

Both scripts below write plain text and print JSON about it. Without an out path, the transcript
goes to `transcripts/<source name>.txt` under the output root; a calling skill passes its own
path. Downloaded audio and segments are scratch files, removed when the run ends.

## Batch transcription

For a finite file or URL:

```bash
"$SKILL_DIR/scripts/transcribe-audio.mjs" \
  "<audio-url-or-file>" ["<out.txt>"]
```

A local path is transcribed in place; a URL is downloaded first, with a 10-minute cap. The file
may be audio or video, since whisper decodes it with ffmpeg and takes the audio track. The script
prints JSON with `transcript` (the out path), `words`, `thin` (true when `words < 1500`) and
`source`. A long recording takes minutes, so run it in the background.

## Live transcription

For an ongoing stream:

```bash
"$SKILL_DIR/scripts/transcribe-live.mjs" \
  "<stream>" ["<out.txt>"] [--segment-seconds 30] [--max-minutes N]
```

`<stream>` is either a direct URL ffmpeg can read (HLS/`.m3u8`, Icecast or radio, RTMP, http) or a
platform live: Twitch and YouTube are resolved with `streamlink` and `yt-dlp`, and X Spaces are
attempted with `yt-dlp`. A video stream works too; ffmpeg keeps the audio and ignores the
picture.

ffmpeg cuts the stream into chunks of `--segment-seconds` (30 by default, whisper's window).
Each completed chunk is transcribed and appended to `<out.txt>`, so the file can be read while
the run continues, and each chunk's text is also echoed to stderr. The run stops when the stream
ends, at `--max-minutes` if given, or on SIGINT (Ctrl-C). In every case it finalizes and
transcribes the open chunk before exiting, then prints a JSON summary with `words`, `chunks` and
the batch fields.

Choose `--max-minutes` before launch unless the user asked for an ongoing stream. Keep the
process handle and read `<out.txt>` for partial results. Wait for a bounded run to exit, or send
SIGINT to stop an ongoing one and let it finalize; never wait on a blocking `tail -f`.

## Reading the transcript

- **No speaker labels.** Whisper emits one stream of text, so attribute quotes to the source,
  not to a named speaker.
- **Only what the audio carries.** Downloaded episode audio often has dynamically inserted
  modern ads that are not part of the original recording, and whisper can loop, repeating a line
  over a garbled or musical stretch. Note both and exclude or repair them when you read.
- **Verbatim, not edited.** It keeps filler and false starts. Compress when you summarise, but
  never claim the transcript says something it does not.
