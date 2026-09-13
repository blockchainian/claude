---
name: transcribe-audio
description: >
  Turn an audio file or audio URL into plain-text words, transcribed locally
  with whisper. Use when you have recorded audio — a podcast episode, an
  interview, a voice memo, a downloaded stream — and need its transcript, and
  the source offers no text of its own. Other skills call this as their
  audio-to-text step.

  NOT for: live/ongoing streams (this reads a finite file end to end), reading
  a text transcript that already exists (fetch it directly), or video where you
  only need the audio track (extract the audio first).
---

# Transcribe audio — local whisper, file or URL to words

One script. Give it an audio file or a URL and an output path; it writes the
transcript as plain text and prints JSON about it.

```bash
python3 "${CLAUDE_PLUGIN_ROOT}/skills/transcribe-audio/scripts/transcribe_audio.py" \
  "<audio-url-or-file>" "<out.txt>"
```

- A local path is transcribed in place; a URL is downloaded first (`curl`,
  10-minute cap).
- It prints JSON with `transcript` (the out path), `words`, `thin`
  (`words < 1500`) and `source`.
- Transcription is **long-running**: the model (~1.5GB, `whisper-large-v3-turbo`)
  downloads on first use, then runs faster than realtime. Run it in the
  **background** and wait for it, rather than blocking a foreground call.

## Requirements

Apple Silicon, and one of:
- `mlx_whisper` on `PATH` (`pip install mlx-whisper`), or
- `uv` on `PATH` (the script runs `uv run --with mlx-whisper` in an ephemeral
  env — no install needed).

If neither is present the script says so and exits; there is no cloud fallback.

## What the transcript is, and is not

- **No speaker labels.** Whisper emits a single stream of text, so attribute
  quotes to the source, not to a named speaker.
- **Whatever the audio actually carries.** Downloaded episode audio often has
  **dynamically-inserted modern ads** that are not part of the original
  recording; whisper can also **loop**, repeating a line over a garbled or
  musical stretch. Note both and exclude or repair them when you read.
- **Verbatim, not edited.** It keeps filler and false starts; compress when you
  summarise, but do not claim the transcript says something it does not.

## Tests

`scripts/test_transcribe.py` covers the command shape and, when `ffmpeg` and a
whisper runner are present, a real end-to-end transcription of a generated clip:

```bash
python3 "${CLAUDE_PLUGIN_ROOT}/skills/transcribe-audio/scripts/test_transcribe.py"
```
