#!/usr/bin/env python3
# ABOUTME: Tests the batch archive fetcher (saving, per-route rate, leaving a refused route, the all-refused
# ABOUTME: error), reading a count out of a capture, and the dated curve. A local HTTP server plays the archive.

import http.server
import importlib.util
import json
import shutil
import sys
import tempfile
import threading
import time
from pathlib import Path

HERE = Path(__file__).parent
fails = []


def check(name, ok, detail=""):
    print(("ok   " if ok else "FAIL ") + name + (f" — {detail}" if detail and not ok else ""))
    if not ok:
        fails.append(name)


def load():
    spec = importlib.util.spec_from_file_location("wayback", HERE / "wayback.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


class Archive(http.server.BaseHTTPRequestHandler):
    """/page/<n> is a capture; /replayed429 is a capture of a 429; /throttled is the archive refusing the caller."""

    def do_GET(self):
        if self.path.startswith("/cdx/search/cdx"):
            rows = [["timestamp", "statuscode"], ["20140115000000", "200"], ["20150715000000", "200"], ["20200915000000", "200"]]
            body = json.dumps(rows if "channel-a" in self.path else []).encode()
            self.send_response(200)
            self.end_headers()
            self.wfile.write(body)
            return
        if self.path.startswith("/web/"):
            self.send_response(200)
            self.send_header("x-archive-orig-date", "then")
            self.end_headers()
            self.wfile.write(CAPTURES[self.path[5:9]].encode())
            return
        code = {"/replayed429": 429, "/throttled": 429}.get(self.path, 200)
        self.send_response(code)
        if self.path != "/throttled":
            self.send_header("x-archive-orig-date", "then")
        self.end_headers()
        self.wfile.write(f"body of {self.path}".encode())

    def log_message(self, *args):
        pass


OLD = '<span class="yt-subscription-button-subscriber-count-branded-horizontal" title="10,490,968 subscribers">10,490,968</span>'
LOCALIZED = '<span class="yt-subscription-button-subscriber-count-branded-horizontal yt-uix-tooltip" title="37 704 014" aria-label="37 704 014 подписчиков">37</span>'
MODERN = ('"gridChannelRenderer":{"subscriberCountText":{"simpleText":"97K"}},"header":{"c4TabbedHeaderRenderer":{"title":"X",'
          '"subscriberCountText":{"runs":[{"text":"105M subscribers"}]}}}')
MODERN_LOCALIZED = '"c4TabbedHeaderRenderer":{"title":"X","subscriberCountText":{"simpleText":"106 Mln di iscritti"}}'
CAPTURES = {"2014": OLD, "2015": LOCALIZED, "2020": MODERN}


def main():
    wb = load()
    check("an exact count is read from the old channel page", wb.extract(OLD)["value"] == 10490968, str(wb.extract(OLD)))
    check("a count with another locale's separators is read", wb.extract(LOCALIZED)["value"] == 37704014, str(wb.extract(LOCALIZED)))
    found = wb.extract(MODERN)
    check("the channel's own rounded count is read, not a listed channel's", found["value"] == 105000000 and found["text"] == "105M subscribers", str(found))
    found = wb.extract(MODERN_LOCALIZED)
    check("a count in words of another language is kept as text for a reader", found["value"] is None and found["text"] == "106 Mln di iscritti", str(found))
    found = wb.extract('"c4TabbedHeaderRenderer":{"subscriberCountText":{"simpleText":"57.216.326 iscritti"}}')
    check("a full count in the header is read whatever the language", found["value"] == 57216326, str(found))
    check("a twitter capture gives the follower count", wb.extract('{"followers_count":342701,"friends_count":10}')["value"] == 342701)
    check("a page with no count gives nothing", wb.extract("<html>subscribe</html>") == {"value": None, "text": None})

    tmp = Path(tempfile.mkdtemp())
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Archive)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f"http://127.0.0.1:{server.server_port}"
    try:
        urls = [f"{base}/page/{n}" for n in range(3)] + [f"{base}/replayed429"]
        start = time.time()
        results = wb.fetch_all(urls, [None], tmp / "a", per_minute=240)
        took = time.time() - start
        check("every url gets a result, in input order", [r["url"] for r in results] == urls, str(results))
        check("a page is saved to the output folder", all(Path(r["file"]).read_text() == f"body of {r['url'][len(base):]}" for r in results[:3]))
        check("a capture of a 429 is a result, not a refusal", results[3]["status"] == 429 and results[3]["file"])
        check("one route keeps to its rate", took >= 0.75, f"{took:.2f}s for 4 requests at 240 a minute")

        seen = []

        def get(route, url):
            seen.append(route)
            if route == "first":
                raise ConnectionRefusedError("refused")
            return wb.http_get(None, url)

        results = wb.fetch_all(urls[:3], ["first", "second"], tmp / "b", per_minute=6000, get=get)
        check("a refused route is left and the others finish the work", len(results) == 3 and all(r["status"] == 200 and r["route"] == 1 for r in results), str(results))
        check("a refused route is not used again", seen.count("first") <= wb.WORKERS_PER_ROUTE, str(seen))

        try:
            wb.fetch_all(urls[:3] + [f"{base}/throttled"], [None], tmp / "c", per_minute=6000)
            check("a 429 from the archive itself is a refusal", False, "no error raised")
        except wb.AllRoutesRefused as error:
            check("a 429 from the archive itself is a refusal", f"{base}/throttled" in error.remaining, str(error.remaining))
            check("the error says how many urls are left", "1 of 4" in str(error), str(error))

        rows = wb.curve([f"example.com/channel-a", "example.com/channel-b"], [None], tmp / "d", per_minute=6000, archive=base)
        check("the curve has one dated row per capture of every address", [(r["date"], r["value"]) for r in rows]
              == [("2014-01-15", 10490968), ("2015-07-15", 37704014), ("2020-09-15", 105000000)], str(rows))
        check("each curve row names its capture and its saved file", rows[0]["url"] == f"{base}/web/20140115000000id_/example.com/channel-a"
              and Path(rows[0]["file"]).is_file(), str(rows[0]))
    finally:
        server.shutdown()
        shutil.rmtree(tmp, ignore_errors=True)

    print(f"\n{len(fails)} failed" if fails else "\nall passed")
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
