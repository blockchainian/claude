#!/usr/bin/env python3
# ABOUTME: Tests the batch archive fetcher: saving pages, the per-route rate, switching routes on a refusal,
# ABOUTME: and the error when every route is refused. Uses a local HTTP server as the archive.

import http.server
import importlib.util
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
        code = {"/replayed429": 429, "/throttled": 429}.get(self.path, 200)
        self.send_response(code)
        if self.path != "/throttled":
            self.send_header("x-archive-orig-date", "then")
        self.end_headers()
        self.wfile.write(f"body of {self.path}".encode())

    def log_message(self, *args):
        pass


def main():
    wb = load()
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
    finally:
        server.shutdown()
        shutil.rmtree(tmp, ignore_errors=True)

    print(f"\n{len(fails)} failed" if fails else "\nall passed")
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
