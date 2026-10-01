#!/usr/bin/env python3
# ABOUTME: Fetches a batch of archived pages in parallel and saves them, at a fixed rate per route (a proxy or
# ABOUTME: the direct connection). A refused route is dropped; when every route is refused it stops with an error.
#
# Usage: wayback.py <out dir> <url>... | wayback.py <out dir> --from <file with one url per line>
# Routes come from the WAYBACK_PROXIES environment variable (comma-separated proxy URLs, one per exit IP).
# Without it there is one route: the direct connection. Prints one JSON line per url: url, status, file, route.
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


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("out")
    parser.add_argument("urls", nargs="*")
    parser.add_argument("--from", dest="source", help="a file with one url per line")
    args = parser.parse_args()
    urls = args.urls + ([line.strip() for line in Path(args.source).read_text().splitlines() if line.strip()] if args.source else [])
    routes = [p.strip() for p in os.environ.get("WAYBACK_PROXIES", "").split(",") if p.strip()] or [None]
    try:
        for result in fetch_all(urls, routes, args.out):
            print(json.dumps(result, ensure_ascii=False))
    except AllRoutesRefused as error:
        print(f"error: {error}", file=sys.stderr)
        print("\n".join(error.remaining), file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
