# ABOUTME: mitmproxy addon that tallies flows per host and prints the table when reading
# ABOUTME: ends. Use offline (mitmdump -nr file.mitm -s hosts.py) to find an app's hosts.
from collections import Counter

from mitmproxy import http

_counts: Counter[str] = Counter()


def response(flow: http.HTTPFlow) -> None:
    _counts[flow.request.pretty_host] += 1


def request(flow: http.HTTPFlow) -> None:
    # Count hosts even when there is no response (blocked, tunnelled, pending).
    if flow.response is None:
        _counts[flow.request.pretty_host] += 0


def done() -> None:
    if not _counts:
        print("no HTTP flows in file")
        return
    width = max(len(h) for h in _counts)
    for host, n in _counts.most_common():
        print(f"{host:<{width}}  {n}")
