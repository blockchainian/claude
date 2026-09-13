# ABOUTME: mitmproxy addon to attribute a shared-domain capture to the app that made each
# ABOUTME: request, by Origin/Referer/app-id header — since one host can serve several apps.
import os

from mitmproxy import http

# Substring to match against a request's Origin, Referer, or *-app-id headers. When set, only
# matching flows are printed (one line each) — pull one app's calls out of a shared capture.
# When empty, on `done` print a table of (host, source) -> count, so the distinct callers of
# each shared host are visible and you can see what to filter on.
SOURCE = os.environ.get("SOURCE", "")

_seen: dict[tuple[str, str], int] = {}


def _source_of(flow: http.HTTPFlow) -> str:
    h = flow.request.headers
    for key in ("origin", "referer"):
        if key in h:
            return h[key]
    for key in h.keys():
        if key.lower().endswith("app-id"):
            return f"{key}={h[key]}"
    return "(none)"


def response(flow: http.HTTPFlow) -> None:
    src = _source_of(flow)
    if SOURCE:
        if SOURCE in src:
            r = flow.response
            status = str(r.status_code) if r else "-"
            print(f"{flow.request.method:6} {status:>3}  "
                  f"{flow.request.pretty_host}{flow.request.path[:70]}  <- {src[:60]}")
    else:
        _seen[(flow.request.pretty_host, src)] = _seen.get((flow.request.pretty_host, src), 0) + 1


def done() -> None:
    if SOURCE or not _seen:
        return
    hw = max(len(h) for h, _ in _seen)
    for (host, src), n in sorted(_seen.items(), key=lambda kv: -kv[1]):
        print(f"{host:<{hw}}  {n:>4}  {src[:80]}")
