#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
# ABOUTME: Deterministic grounding for an App Store reviews JSON — rating distribution, averages, geography, dates.
# ABOUTME: Also splits every review's text into neg/mid/pos files so the agent can read all of them, not sample.
"""Ground an App Store reviews dataset with reproducible facts.

Input is a reviews JSON with a top-level `reviews` array whose items carry
`rating` (1-5), `title`, `body`, `date`, `country` and optionally
`developerResponseBody`. Prints a stats JSON to stdout. With --dump-dir, also
writes neg.txt (1-2 star), mid.txt (3 star) and pos.txt (4-5 star), one review
per line as `[rating|country] title :: body`, so the agent reads the full text.

Nothing here interprets — it only counts. Themes and quotes are the agent's job.
"""
from __future__ import annotations

import argparse
import collections
import json
import os
import sys


def load(path: str) -> list[dict]:
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    reviews = data.get("reviews", data if isinstance(data, list) else [])
    if not reviews:
        sys.exit(f"no reviews found in {path}")
    return reviews


def dump(path: str, items: list[dict]) -> None:
    with open(path, "w", encoding="utf-8") as f:
        for x in items:
            title = (x.get("title") or "").replace("\n", " ")
            body = (x.get("body") or "").replace("\n", " ")
            f.write(f"[{x.get('rating')}|{x.get('country')}] {title} :: {body}\n")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("json", help="path to the reviews JSON")
    ap.add_argument("--dump-dir", help="write neg/mid/pos text dumps here")
    args = ap.parse_args()

    r = load(args.json)
    for x in r:
        x["rating"] = int(x.get("rating") or 0)
    ratings = collections.Counter(x["rating"] for x in r)
    total = len(r)
    avg = sum(x["rating"] for x in r) / total if total else 0
    countries = collections.Counter(x.get("country") for x in r)
    us = countries.get("us", 0)
    dates = sorted(x.get("date") or "" for x in r if x.get("date"))
    neg = [x for x in r if x["rating"] <= 2]
    mid = [x for x in r if x["rating"] == 3]
    pos = [x for x in r if x["rating"] >= 4]

    out = {
        "total": total,
        "ratings": {str(k): ratings.get(k, 0) for k in range(1, 6)},
        "avg": round(avg, 2),
        "us": us,
        "nonUs": total - us,
        "usPct": round(100 * us / total) if total else 0,
        "countries": len(countries),
        "topCountries": countries.most_common(8),
        "dateRange": [dates[0][:10], dates[-1][:10]] if dates else None,
        "developerResponses": sum(1 for x in r if x.get("developerResponseBody")),
        "n_dislike_1_3": total - len(pos),
        "n_like_4_5": len(pos),
    }
    print(json.dumps(out, ensure_ascii=False, indent=2))

    if args.dump_dir:
        os.makedirs(args.dump_dir, exist_ok=True)
        dump(os.path.join(args.dump_dir, "neg.txt"), sorted(neg, key=lambda x: x["rating"]))
        dump(os.path.join(args.dump_dir, "mid.txt"), mid)
        dump(os.path.join(args.dump_dir, "pos.txt"), pos)
        print(f"\nwrote neg({len(neg)}) mid({len(mid)}) pos({len(pos)}) to {args.dump_dir}", file=sys.stderr)


if __name__ == "__main__":
    main()
