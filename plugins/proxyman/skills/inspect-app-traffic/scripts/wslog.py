# ABOUTME: mitmproxy addon printing WebSocket frames, optionally filtered to one host.
# ABOUTME: Control with `--set wshost=<substring>` and `--set wsmax=<chars>`.
from mitmproxy import ctx, http


def load(loader) -> None:
    loader.add_option(
        "wshost", str, "",
        "Substring; only WebSocket frames whose host matches are printed. Empty = all hosts.")
    loader.add_option(
        "wsmax", int, 220,
        "Max characters of each frame body to print.")


def websocket_message(flow: http.HTTPFlow) -> None:
    host = ctx.options.wshost
    if host and host not in flow.request.pretty_host:
        return
    if not flow.websocket:
        return
    m = flow.websocket.messages[-1]
    arrow = "SEND->" if m.from_client else "<-RECV"
    body = m.content.decode("utf-8", "replace").replace("\n", " ")
    print(f"{flow.request.pretty_host} {arrow} {body[:ctx.options.wsmax]}")
