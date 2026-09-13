#!/usr/bin/env python3
# ABOUTME: Tests digest source-fetching (articles + transcripts) and the store.
# ABOUTME: Covers main-content extraction, audio detection, save, and take-aways.

import importlib.util
import os
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).parent
fails = []

ARTICLE = """<html><head><title>Ignore</title></head><body>
<nav>Home About Subscribe Newsletter</nav>
<article>
  <h1>The Headline</h1>
  <p>The quick brown fox jumps over the lazy dog in a wide green meadow.</p>
  <p>A second paragraph adds detail about the fox, the dog, and the meadow.</p>
</article>
<footer>Copyright 2026 Example</footer></body></html>"""


def load(name):
    spec = importlib.util.spec_from_file_location(name, HERE / f"{name}.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def check(name, cond, detail=""):
    print(f"{'PASS' if cond else 'FAIL'}: {name}" + (f"  -- {detail}" if not cond and detail else ""))
    if not cond:
        fails.append(name)


def caught(fn):
    try:
        fn()
        return False
    except SystemExit:
        return True


def main():
    fs = load("fetch_source")

    # --- audio detection (unchanged, now in fetch_source) ---
    check("mp3 url is audio", fs.is_audio_url("https://x.com/a/b.mp3"))
    check("html page is not audio", not fs.is_audio_url("https://site.com/posts/hello"))
    check("finds enclosure audio", fs.find_audio_url(
        '<enclosure url="https://e.com/ep.m4a" length="1"/>', "https://e.com")
        == "https://e.com/ep.m4a")
    check("no audio -> None", fs.find_audio_url("<p>text</p>", "https://e.com") is None)

    # --- main-content extraction: keeps the article's prose (trafilatura or fallback) ---
    text = fs.extract_main_text(ARTICLE, "https://site.com/posts/fox")
    check("extract keeps the article prose", "quick brown fox" in text, text[:120])
    check("extract keeps the second paragraph", "second paragraph" in text.lower(), text[:200])

    # fallback is still available and text-only
    plain = fs.html_to_text(ARTICLE)
    check("html_to_text still works", "quick brown fox" in plain)

    # slug for an article URL
    check("article slug is distinctive", fs.slugify("https://site.com/posts/why-x-wins") == "why-x-wins",
          fs.slugify("https://site.com/posts/why-x-wins"))

    # --- store: an article draft (no 'show') saves ---
    root = Path(tempfile.mkdtemp())
    os.environ["PODCAST_HIGHLIGHTS_DIR"] = str(root)
    store = load("store")

    draft = root / "draft.md"
    draft.write_text(
        "---\ntitle: Why X Wins\nurl: https://site.com/posts/why-x-wins\n"
        "slug: why-x-wins\nsource: Example Blog\ntopics: [x]\n---\n"
        "# Why X Wins\n\n## Point\n- a point\n", encoding="utf-8")
    store.cmd_save(str(draft))
    saved = store.EPISODES / "why-x-wins.md"
    check("article (no show) saved", saved.is_file(), str(saved))
    if saved.is_file():
        meta, _ = store.parse_frontmatter(saved.read_text(encoding="utf-8"))
        check("save stamped date", bool(meta.get("saved")))

    # a draft with only title+url (no source/show) also saves
    d2 = root / "d2.md"
    d2.write_text("---\ntitle: Bare\nurl: https://site.com/bare\nslug: bare\n---\n# Bare\n",
                  encoding="utf-8")
    check("bare title+url draft saves", not caught(lambda: store.cmd_save(str(d2))))

    # missing url is still refused
    d3 = root / "d3.md"
    d3.write_text("---\ntitle: NoUrl\nslug: nourl\n---\n# NoUrl\n", encoding="utf-8")
    check("missing url is refused", caught(lambda: store.cmd_save(str(d3))))

    # --- take-aways still work on the stored article ---
    store.cmd_takeaway(str(saved), "add", None, "keep the thesis, not the timeline")
    check("take-away added", store.current_takeaways(saved) == ["keep the thesis, not the timeline"],
          store.current_takeaways(saved))
    store.cmd_takeaway(str(saved), "revise", 1, "keep the thesis")
    check("take-away revised", store.current_takeaways(saved) == ["keep the thesis"],
          store.current_takeaways(saved))

    print()
    if fails:
        print(f"{len(fails)} FAILED: {fails}")
        sys.exit(1)
    print("ALL PASSED")


if __name__ == "__main__":
    main()
