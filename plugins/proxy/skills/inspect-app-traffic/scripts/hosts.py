# ABOUTME: mitmproxy addon that tallies flows per host and prints the table when reading
# ABOUTME: ends. Scoped by `--set since=`/`--set host=`; use it to find an app's hosts.
import re
from collections import Counter

from mitmproxy import ctx, http

_counts: Counter[str] = Counter()


def load(loader) -> None:
    loader.add_option("since", str, "", "only flows started at or after this epoch time")
    loader.add_option("host", str, "", "regex; only hosts matching are counted")


def _passes(flow: http.HTTPFlow) -> bool:
    if (flow.request.timestamp_start or 0) < float(ctx.options.since or 0):
        return False
    h = ctx.options.host
    return not h or bool(re.search(h, flow.request.pretty_host))


def request(flow: http.HTTPFlow) -> None:
    if _passes(flow):
        _counts[flow.request.pretty_host] += 1


def done() -> None:
    if not _counts:
        print("no HTTP flows in the capture window")
        return
    width = max(len(h) for h in _counts)
    for host, n in _counts.most_common():
        print(f"{host:<{width}}  {n}")
