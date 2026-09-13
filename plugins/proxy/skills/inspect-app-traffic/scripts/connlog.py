# ABOUTME: hub-side addon that logs, with a timestamp, each proxy client connection and each
# ABOUTME: request host, so `capture.py check`/`stop` count a capture's traffic from the log.
import time

from mitmproxy import http


def client_connected(client) -> None:
    # Fires when anything points at the hub port, before any HTTP — the signal that the proxy
    # (Zero Omega, a phone, a system proxy) is actually enabled and connecting.
    print(f"PROXY_CLIENT_CONNECTED {time.time():.3f}", flush=True)


def request(flow: http.HTTPFlow) -> None:
    # One line per request, with the host, so a capture's window and hosts can be counted from
    # the log alone — no need to re-parse the whole shared flow file.
    print(f"PROXY_REQUEST {time.time():.3f} {flow.request.pretty_host}", flush=True)
