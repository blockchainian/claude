#!/usr/bin/env python3
# ABOUTME: Fetches archived pages in one parallel batch at a fixed rate per route (a proxy or the direct
# ABOUTME: connection), and builds a dated follower curve from every monthly capture of a profile page.
#
# Usage: wayback.py fetch <out dir> <url>... [--from <file with one url per line>]
#        wayback.py curve <out dir> <profile url>...     (every address the profile has had)
# Routes come from the WAYBACK_PROXIES environment variable (comma-separated proxy URLs, one per exit IP).
# Without it there is one route: the direct connection. A refused route is dropped; when every route is refused
# the run stops with an error. fetch prints one JSON line per url (url, status, file, route). curve prints one
# JSON line per capture (date, value, text, url, file): value is the count when it could be read, text is the
# page's own wording when it is rounded or in another language, and both are null when the page shows no count.
import argparse
import json
import os
import queue
import re
import sys
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path

PER_MINUTE = 30  # requests per route; the archive refuses an IP well above this
WORKERS_PER_ROUTE = 3  # a page takes seconds to arrive, so one worker alone cannot reach the rate
ARCHIVE = "https://web.archive.org"
EXACT = [  # patterns whose first group is the full count, digits with any locale's separators
    re.compile(r'subscriber-count[^>]*title="([\d][\d,.\s\u00a0]*)'),
    re.compile(r'"followers_count":(\d+)'),
    re.compile(r'title="([\d][\d,.\s\u00a0]*) Followers"'),
    re.compile(r'([\d][\d,]{4,}) subscribers'),
]
# The channel's own header on the script-rendered page; other channels listed on the page have counts too.
HEADER = re.compile(r'"c4TabbedHeaderRenderer".*?"subscriberCountText":\{(?:"simpleText":"|"runs":\[\{"text":")([^"]+)"', re.S)
FULL = re.compile(r"^\d{1,3}(?:[.,\s\u00a0]\d{3})+")  # a whole count with thousands separators
ROUNDED = re.compile(r"^([\d.]+)([KMB]) subscribers$")


class AllRoutesRefused(Exception):
    """Every route was refused before the batch finished."""

    def __init__(self, remaining, total):
        super().__init__(f"every route was refused by the archive; {len(remaining)} of {total} urls not fetched")
        self.remaining = remaining


class Refused(Exception):
    """The archive (or the proxy) turned this route away."""


def http_get(route, url):
    """(status, body) for one url through one route. Raises Refused when the archive itself answers 429."""
    handlers = [urllib.request.ProxyHandler({"http": route, "https": route})] if route else []
    request = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
    try:
        with urllib.request.build_opener(*handlers).open(request, timeout=60) as response:
            return response.status, response.read()
    except urllib.error.HTTPError as error:
        # A capture of a page that answered 429 at the time carries the archive's replay headers; a bare 429 is
        # the archive limiting the caller.
        replayed = any(name.lower().startswith("x-archive-orig-") for name in error.headers)
        if error.code == 429 and not replayed:
            raise Refused(url) from error
        return error.code, error.read()


def fetch_all(urls, routes, out, per_minute=PER_MINUTE, get=http_get):
    """Fetch every url, save each body under out, and return one result per url in input order."""
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    todo = queue.Queue()
    for item in enumerate(urls):
        todo.put(item)
    results, refused, lock = {}, set(), threading.Lock()
    next_start = [0.0] * len(routes)

    def work(index):
        while index not in refused and len(results) < len(urls):
            try:
                position, url = todo.get(timeout=0.2)
            except queue.Empty:
                continue
            with lock:  # space this route's requests evenly
                start = max(time.monotonic(), next_start[index])
                next_start[index] = start + 60 / per_minute
            time.sleep(max(0.0, start - time.monotonic()))
            try:
                status, body = get(routes[index], url)
            except (Refused, OSError):  # refused connection, proxy failure, timeout
                refused.add(index)
                todo.put((position, url))
                return
            path = out / (f"{position:03d}-" + re.sub(r"[^A-Za-z0-9]+", "-", url)[-120:].strip("-") + ".html")
            path.write_bytes(body)
            with lock:
                results[position] = {"url": url, "status": status, "file": str(path), "route": index}

    threads = [threading.Thread(target=work, args=(index,)) for index in range(len(routes)) for _ in range(WORKERS_PER_ROUTE)]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    if len(results) < len(urls):
        raise AllRoutesRefused([url for position, url in enumerate(urls) if position not in results], len(urls))
    return [results[position] for position in range(len(urls))]


def extract(page):
    """The follower or subscriber count a capture shows: {"value": int or None, "text": the page's wording or None}."""
    for pattern in EXACT:
        match = pattern.search(page)
        if match:
            return {"value": int(re.sub(r"\D", "", match.group(1))), "text": match.group(1).strip()}
    match = HEADER.search(page)
    if not match:
        return {"value": None, "text": None}
    text = match.group(1)
    full, rounded = FULL.match(text), ROUNDED.match(text)
    if full:
        return {"value": int(re.sub(r"\D", "", full.group())), "text": text}
    value = round(float(rounded.group(1)) * {"K": 1e3, "M": 1e6, "B": 1e9}[rounded.group(2)]) if rounded else None
    return {"value": value, "text": text}


def curve(addresses, routes, out, per_minute=PER_MINUTE, archive=ARCHIVE):
    """One row per monthly capture of every address, oldest first, with the count each capture shows."""
    listings = [f"{archive}/cdx/search/cdx?url={address}&output=json&fl=timestamp,statuscode"
                "&filter=statuscode:200&collapse=timestamp:6" for address in addresses]
    out = Path(out)
    captures = []
    for address, listing in zip(addresses, fetch_all(listings, routes, out / "lists", per_minute)):
        rows = json.loads(Path(listing["file"]).read_text() or "[]")
        captures += [f"{archive}/web/{stamp}id_/{address}" for stamp, _ in rows[1:]]
    rows = []
    for page in fetch_all(captures, routes, out, per_minute):
        stamp = re.search(r"/web/(\d{8})", page["url"]).group(1)
        found = extract(Path(page["file"]).read_text(errors="replace")) if page["status"] == 200 else {"value": None, "text": None}
        rows.append({"date": f"{stamp[:4]}-{stamp[4:6]}-{stamp[6:]}", **found, "url": page["url"], "file": page["file"]})
    return sorted(rows, key=lambda row: row["date"])


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("fetch")
    p.add_argument("out")
    p.add_argument("urls", nargs="*")
    p.add_argument("--from", dest="source", help="a file with one url per line")
    p = sub.add_parser("curve")
    p.add_argument("out")
    p.add_argument("addresses", nargs="+")
    args = parser.parse_args()
    routes = [p.strip() for p in os.environ.get("WAYBACK_PROXIES", "").split(",") if p.strip()] or [None]
    try:
        if args.cmd == "curve":
            results = curve(args.addresses, routes, args.out)
        else:
            urls = args.urls + ([line.strip() for line in Path(args.source).read_text().splitlines() if line.strip()] if args.source else [])
            results = fetch_all(urls, routes, args.out)
        for result in results:
            print(json.dumps(result, ensure_ascii=False))
    except AllRoutesRefused as error:
        print(f"error: {error}", file=sys.stderr)
        print("\n".join(error.remaining), file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
