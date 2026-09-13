# ABOUTME: mitmproxy addon printing WebSocket frames, optionally filtered to one host.
# ABOUTME: Set WSHOST to a substring of the host to show only that app's frames.
import os

from mitmproxy import http

HOSTFILTER = os.environ.get("WSHOST", "")
MAXLEN = int(os.environ.get("WSMAX", "220"))


def websocket_message(flow: http.HTTPFlow) -> None:
    if HOSTFILTER and HOSTFILTER not in flow.request.pretty_host:
        return
    if not flow.websocket:
        return
    m = flow.websocket.messages[-1]
    arrow = "SEND->" if m.from_client else "<-RECV"
    body = m.content.decode("utf-8", "replace").replace("\n", " ")
    print(f"{flow.request.pretty_host} {arrow} {body[:MAXLEN]}")
