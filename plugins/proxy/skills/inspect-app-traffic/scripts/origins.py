# ABOUTME: mitmproxy addon to attribute a shared-domain capture to the app that made each
# ABOUTME: request, by Origin/Referer/app-id header — since one host can serve several apps.
import re

from mitmproxy import ctx, http


def load(loader) -> None:
    loader.add_option("since", str, "", "only flows started at or after this epoch time")
    loader.add_option("host", str, "", "regex; only flows whose host matches are considered")
    # Controlled with `--set source=<substring>`. When set, only flows whose caller
    # (Origin / Referer / *-app-id header) contains it are printed, one line each — pull one
    # app's calls out of a shared capture. When empty, on `done` a table of (host, caller) ->
    # count is printed instead, so the distinct callers of each shared host are visible.
    loader.add_option(
        "source", str, "",
        "Substring matched against a request's Origin/Referer/*-app-id header. "
        "Empty lists callers per host; set pulls one caller's flows.")


_seen: dict[tuple[str, str], int] = {}


def _passes(flow: http.HTTPFlow) -> bool:
    if (flow.request.timestamp_start or 0) < float(ctx.options.since or 0):
        return False
    h = ctx.options.host
    return not h or bool(re.search(h, flow.request.pretty_host))


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
    if not _passes(flow):
        return
    src = _source_of(flow)
    source = ctx.options.source
    if source:
        if source in src:
            r = flow.response
            status = str(r.status_code) if r else "-"
            print(f"{flow.request.method:6} {status:>3}  "
                  f"{flow.request.pretty_host}{flow.request.path[:70]}  <- {src[:60]}")
    else:
        _seen[(flow.request.pretty_host, src)] = _seen.get((flow.request.pretty_host, src), 0) + 1


def done() -> None:
    if ctx.options.source or not _seen:
        return
    hw = max(len(h) for h, _ in _seen)
    for (host, src), n in sorted(_seen.items(), key=lambda kv: -kv[1]):
        print(f"{host:<{hw}}  {n:>4}  {src[:80]}")
