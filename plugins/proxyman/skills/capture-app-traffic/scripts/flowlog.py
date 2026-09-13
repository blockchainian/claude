# ABOUTME: mitmproxy addon printing one clean line per captured flow: method, status,
# ABOUTME: real host+path, and response content-type. Works live (-s) or offline (-nr file -s).
from mitmproxy import http


def response(flow: http.HTTPFlow) -> None:
    r = flow.response
    ct = r.headers.get("content-type", "").split(";")[0] if r else ""
    status = str(r.status_code) if r else "-"
    print(f"{flow.request.method:6} {status:>3}  "
          f"{flow.request.pretty_host}{flow.request.path[:80]}  [{ct}]")
