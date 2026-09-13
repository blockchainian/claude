# ABOUTME: mitmproxy addon printing one clean line per captured flow: method, status,
# ABOUTME: real host+path, and response content-type. Scoped by `--set since=`/`--set host=`.
import re

from mitmproxy import ctx, http


def load(loader) -> None:
    loader.add_option("since", str, "", "only flows started at or after this epoch time")
    loader.add_option("host", str, "", "regex; only flows whose host matches are shown")


def _passes(flow: http.HTTPFlow) -> bool:
    if (flow.request.timestamp_start or 0) < float(ctx.options.since or 0):
        return False
    h = ctx.options.host
    return not h or bool(re.search(h, flow.request.pretty_host))


def response(flow: http.HTTPFlow) -> None:
    if not _passes(flow):
        return
    r = flow.response
    ct = r.headers.get("content-type", "").split(";")[0] if r else ""
    status = str(r.status_code) if r else "-"
    print(f"{flow.request.method:6} {status:>3}  "
          f"{flow.request.pretty_host}{flow.request.path[:80]}  [{ct}]")
