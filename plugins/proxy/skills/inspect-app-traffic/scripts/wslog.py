# ABOUTME: mitmproxy addon printing WebSocket frames, scoped to a capture window and host.
# ABOUTME: Control with `--set since=`, `--set host=<regex>`, and `--set wsmax=<chars>`.
import re

from mitmproxy import ctx, http


def load(loader) -> None:
    loader.add_option("since", str, "", "only sockets opened at or after this epoch time")
    loader.add_option("host", str, "", "regex; only frames whose host matches are printed")
    loader.add_option("wsmax", int, 220, "max characters of each frame body to print")


def websocket_message(flow: http.HTTPFlow) -> None:
    if (flow.request.timestamp_start or 0) < float(ctx.options.since or 0):
        return
    h = ctx.options.host
    if h and not re.search(h, flow.request.pretty_host):
        return
    if not flow.websocket:
        return
    m = flow.websocket.messages[-1]
    arrow = "SEND->" if m.from_client else "<-RECV"
    body = m.content.decode("utf-8", "replace").replace("\n", " ")
    print(f"{flow.request.pretty_host} {arrow} {body[:ctx.options.wsmax]}")
