# ABOUTME: capture-side addon that logs proxy client connections and intercepted requests to
# ABOUTME: the run log, so `capture.py check` can tell "proxy enabled" from "target host missed".
from mitmproxy import http


def client_connected(client) -> None:
    # Fires when anything points at the proxy port, before any HTTP — the signal that the
    # proxy (Zero Omega, a phone, a system proxy) is actually enabled and connecting.
    print("PROXY_CLIENT_CONNECTED", flush=True)


def request(flow: http.HTTPFlow) -> None:
    # With allow_hosts set, only target hosts are intercepted, so this fires for target
    # traffic only; non-target hosts pass through as raw tunnels and never reach here.
    print(f"PROXY_REQUEST {flow.request.pretty_host}", flush=True)
