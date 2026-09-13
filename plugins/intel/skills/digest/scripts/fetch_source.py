#!/usr/bin/env python3
# ABOUTME: Fetches the readable text of a source URL and writes it as plain text.
# ABOUTME: Handles articles, podcast transcript pages, YouTube subtitles, and audio.

import html
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from urllib.parse import urljoin, urlparse

UA = ("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 "
      "(KHTML, like Gecko) Chrome/126 Safari/537.36")

ROOT = Path(os.environ.get("PODCAST_HIGHLIGHTS_DIR",
                           Path.home() / ".claude" / "podcast-highlights"))
WORK = ROOT / ".work"

# Path segments that name no particular episode, so cannot identify one.
GENERIC = {"transcript", "transcripts", "episode", "episodes", "podcast",
           "podcasts", "show", "shows", "index", "home", "default", "p"}

BLOCK = r"(?:p|div|li|h[1-6]|tr|section|article|blockquote)"


def distinctive(segment):
    return segment.lower() not in GENERIC and not segment.isdigit()


def slugify(url):
    p = urlparse(url)
    if p.query and "v=" in p.query:
        return clean(re.sub(r".*v=([\w-]+).*", r"\1", p.query))
    parts = [re.sub(r"\.(html?|php)$", "", s) for s in p.path.split("/") if s]
    picked = []
    for segment in reversed(parts):
        picked.insert(0, segment)
        if distinctive(segment) or len(picked) == 3:
            break
    if not any(distinctive(s) for s in picked):
        picked.insert(0, re.sub(r"^www\.", "", p.netloc))
    return clean("-".join(picked))


def clean(text):
    return re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-") or "episode"


def is_youtube(url):
    host = urlparse(url).netloc.lower()
    return "youtube.com" in host or "youtu.be" in host


AUDIO_EXT = r"\.(?:mp3|m4a|aac|ogg|oga|wav|flac)"


def is_audio_url(url):
    return re.search(AUDIO_EXT + r"$", urlparse(url).path, re.I) is not None


def find_audio_url(raw, base):
    """The episode's audio file linked from a page, or None."""
    patterns = [
        r'<source[^>]+src=["\']([^"\']+)["\'][^>]*type=["\']audio/',
        r'<audio[^>]+src=["\']([^"\']+)["\']',
        r'<meta[^>]+property=["\']og:audio["\'][^>]+content=["\']([^"\']+)["\']',
        r'<enclosure[^>]+url=["\']([^"\']+)["\']',
        r'(https?://[^"\'\s]+' + AUDIO_EXT + r'(?:\?[^"\'\s]*)?)',
    ]
    for pat in patterns:
        m = re.search(pat, raw, re.I)
        if m:
            return urljoin(base, html.unescape(m.group(1)))
    return None


def html_to_text(raw):
    raw = re.sub(r"(?is)<(script|style|noscript|svg|head)[^>]*>.*?</\1>", " ", raw)
    raw = re.sub(r"(?is)<br\s*/?>", "\n", raw)
    raw = re.sub(rf"(?is)</?{BLOCK}\b[^>]*>", "\n", raw)
    text = html.unescape(re.sub(r"(?s)<[^>]+>", " ", raw))
    lines = [re.sub(r"[ \t]+", " ", l).strip() for l in text.split("\n")]
    return "\n".join(l for l in lines if l)


MIN_MAIN_WORDS = 50


def trafilatura_text(raw, url):
    """The page's main article text via trafilatura, run in-process or through
    uv's ephemeral env, or None when neither is available or it finds nothing."""
    try:
        import trafilatura
        return trafilatura.extract(raw, url=url)
    except Exception:
        pass
    if shutil.which("uv"):
        res = subprocess.run(
            ["uv", "run", "--with", "trafilatura", "python3", "-c",
             "import sys,trafilatura;print(trafilatura.extract(sys.stdin.read()) or '')"],
            input=raw, capture_output=True, text=True)
        if res.returncode == 0:
            return res.stdout
    return None


def extract_main_text(raw, url):
    """The readable body of a page: trafilatura's main-content extraction when it
    yields real prose (strips nav, sidebars, footers), else a plain tag-strip
    that keeps every block of text — the safe floor for oddly-built pages."""
    main = trafilatura_text(raw, url)
    if main and len(main.split()) >= MIN_MAIN_WORDS:
        return main.strip()
    return html_to_text(raw)


def vtt_to_text(vtt):
    out, seen_last = [], None
    for line in vtt.splitlines():
        line = line.strip()
        if (not line or line.startswith(("WEBVTT", "Kind:", "Language:", "NOTE"))
                or "-->" in line or re.fullmatch(r"\d+", line)):
            continue
        line = re.sub(r"<[^>]+>", "", line).strip()
        if line and line != seen_last:
            out.append(line)
            seen_last = line
    return "\n".join(out)


def rank_sub(path):
    """Prefer plain English, then a regional variant, then anything else."""
    parts = path.name.split(".")
    lang = parts[-2].lower() if len(parts) >= 3 else ""
    if lang == "en":
        return (0, lang)
    if lang.startswith("en-") and not lang.endswith("-orig"):
        return (1, lang)
    return (2, lang)


def fetch_youtube(url, workdir):
    if shutil.which("yt-dlp") is None:
        raise SystemExit("yt-dlp is not installed (brew install yt-dlp)")
    tmp = Path(tempfile.mkdtemp(dir=workdir))
    stderr = ""
    for auto in (False, True):
        cmd = (["yt-dlp", "--write-sub"] + (["--write-auto-sub"] if auto else [])
               + ["--sub-lang", "en.*", "--sub-format", "vtt", "--skip-download",
                  "-o", str(tmp / "%(id)s"), url])
        res = subprocess.run(cmd, capture_output=True, text=True)
        stderr = res.stderr.strip()[-500:] or stderr
        subs = sorted(tmp.glob("*.vtt"), key=rank_sub)
        if subs:
            text = subs[0].read_text(encoding="utf-8", errors="replace")
            return vtt_to_text(text), "auto" if auto else "manual"
    raise SystemExit("no subtitles found via yt-dlp: " + (stderr or "unknown error"))


def fetch_page(url):
    res = subprocess.run(["curl", "-sL", "--max-time", "60", "-A", UA,
                          "-w", "\n%{http_code}", url],
                         capture_output=True, text=True)
    if res.returncode != 0:
        raise SystemExit(f"curl failed for {url}: {res.stderr.strip()[-300:]}")
    body, _, status = res.stdout.rpartition("\n")
    if not status.isdigit():
        raise SystemExit(f"no response from {url}")
    if int(status) >= 400:
        raise SystemExit(f"HTTP {status} from {url} — nothing was fetched")
    if not body.strip():
        raise SystemExit(f"empty response from {url}")
    return body


def main():
    if len(sys.argv) < 2:
        raise SystemExit("usage: fetch_transcript.py <url>")
    url = sys.argv[1]
    slug = slugify(url)
    workdir = WORK / slug
    workdir.mkdir(parents=True, exist_ok=True)

    audio_url = None
    if is_youtube(url):
        text, subtitles = fetch_youtube(url, workdir)
    elif is_audio_url(url):
        text, subtitles, audio_url = "", None, url
    else:
        raw = fetch_page(url)
        text, subtitles = extract_main_text(raw, url), None
        audio_url = find_audio_url(raw, url)

    path = workdir / "transcript.txt"
    path.write_text(text, encoding="utf-8")
    words = len(text.split())
    print(json.dumps({
        "slug": slug,
        "url": url,
        "transcript": str(path),
        "draft": str(workdir / "draft.md"),
        "lines": text.count("\n") + 1,
        "words": words,
        "thin": words < 1500,
        "subtitles": subtitles,
        "audio_url": audio_url,
    }, indent=2))


if __name__ == "__main__":
    main()
