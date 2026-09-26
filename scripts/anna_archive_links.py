#!/usr/bin/env python3
"""Find the most downloaded EPUB on one Anna's Archive search page.

Only fetches search, metadata, and download-option pages. Never fetches a file.
"""

import argparse
import concurrent.futures
import json
import os
import re
import sys
from html import unescape
from html.parser import HTMLParser
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.parse import urlencode, urljoin, urlparse
from urllib.request import HTTPRedirectHandler, Request, build_opener


BASE = "https://annas-archive.pk"
MD5_LINK = re.compile(r"^/md5/([0-9a-f]{32})/?$")
SLOW_LINK = re.compile(r"^/slow_download/([0-9a-f]{32})/\d+/\d+/?$")


class Links(HTMLParser):
    def __init__(self):
        super().__init__()
        self.anchors = []
        self.stack = []
        self.current = None
        self.list_count = 0
        self.active_list = None
        self.current_li = None
        self.copy_urls = []

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == "div" and "js-aarecord-list-outer" in attrs.get("class", "").split():
            self.list_count += 1
            if self.active_list is None:
                self.active_list = (self.list_count, len(self.stack))
        if tag == "li":
            self.current_li = {"text": ""}
        if tag == "a":
            self.current = {
                "href": attrs.get("href", ""),
                "text": "",
                "parents": tuple(self.stack),
                "list_index": self.active_list[0] if self.active_list else None,
                "li": self.current_li,
            }
        if tag not in ("area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "wbr"):
            self.stack.append((tag, attrs.get("class", "")))

    def handle_data(self, data):
        if self.current is not None:
            self.current["text"] += data
        if self.current_li is not None:
            self.current_li["text"] += data
        if self.stack and self.stack[-1][0] == "span" and "bg-gray-200" in self.stack[-1][1].split():
            url = unescape(data.strip())
            if urlparse(url).scheme in ("http", "https"):
                self.copy_urls.append(url)

    def handle_endtag(self, tag):
        if tag == "a" and self.current is not None:
            self.current["text"] = " ".join(self.current["text"].split())
            self.anchors.append(self.current)
            self.current = None
        for i in range(len(self.stack) - 1, -1, -1):
            if self.stack[i][0] == tag:
                del self.stack[i:]
                break
        if self.active_list and len(self.stack) <= self.active_list[1]:
            self.active_list = None
        if tag == "li":
            self.current_li = None


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        return None


def get(url, *, follow=True):
    opener = build_opener() if follow else build_opener(NoRedirect)
    request = Request(url, headers={"User-Agent": "anna-archive-links/1.0", "Accept-Language": "en"})
    try:
        with opener.open(request, timeout=15) as response:
            body = response.read(3_000_001)
            if len(body) > 3_000_000:
                raise RuntimeError("页面超过 3 MB，已停止读取")
            return response.status, response.headers.get("Content-Type", ""), body.decode("utf-8", "replace")
    except HTTPError as error:
        return error.code, error.headers.get("Content-Type", ""), error.read(30_000).decode("utf-8", "replace")
    except URLError as error:
        raise RuntimeError(f"请求失败：{error.reason}") from error


def checked_html(status, body, label):
    if "DDoS-Guard" in body or "Checking your browser" in body:
        raise RuntimeError(f"{label}遇到浏览器验证；可用浏览器保存页面后传入对应的 --*-html 文件")
    if status != 200:
        raise RuntimeError(f"{label}返回 HTTP {status}")


def parse_links(body):
    parser = Links()
    parser.feed(body)
    return parser


def search_results(body):
    records = {}
    for anchor in parse_links(body).anchors:
        match = MD5_LINK.fullmatch(anchor["href"])
        if match and anchor["list_index"] == 1 and anchor["text"]:
            records.setdefault(match.group(1), anchor["text"])
    return records


def metric(base, md5):
    status, _, body = get(f"{base}/dyn/md5/inline_info/{md5}")
    if status != 200:
        raise RuntimeError(f"指标接口返回 HTTP {status}")
    try:
        count = json.loads(body)["downloads_total"]
    except (ValueError, KeyError, TypeError) as error:
        raise RuntimeError("指标接口未返回 downloads_total") from error
    if type(count) is not int or count < 0:
        raise RuntimeError("downloads_total 无效")
    return count


def fast_url(base, md5):
    query = {"md5": md5, "key": os.environ.get("ANNA_SECRET_KEY", "")}
    status, _, body = get(f"{base}/dyn/api/fast_download.json?{urlencode(query)}", follow=False)
    try:
        data = json.loads(body)
    except ValueError:
        return {"status": status, "error": "接口没有返回 JSON"}
    url = data.get("download_url")
    if status in (200, 204) and isinstance(url, str) and urlparse(url).scheme in ("http", "https"):
        return {"status": status, "url": url}
    return {"status": status, "error": data.get("error", "没有返回下载链接")}


def slow_url(base, md5, detail_html=None, saved_slow_html=None):
    if detail_html is None:
        status, _, detail_html = get(f"{base}/md5/{md5}")
        checked_html(status, detail_html, "详情页")
    paths = [a for a in parse_links(detail_html).anchors if (match := SLOW_LINK.fullmatch(a["href"])) and match.group(1) == md5]
    if not paths:
        return {"error": "详情页没有慢速下载入口"}

    no_waitlist = next((a for a in paths if a["li"] and "no waitlist" in a["li"]["text"].lower()), paths[0])
    entry = urljoin(base, no_waitlist["href"])
    status, _, body = get(entry, follow=False)
    if status in (301, 302, 303, 307, 308):
        return {"entry": entry, "status": status, "error": "入口发生跳转，未跟随"}

    def direct_url(html):
        links = parse_links(html)
        for anchor in links.anchors:
            href = anchor["href"]
            is_download_paragraph = any(tag == "p" and "text-xl" in classes and "font-bold" in classes for tag, classes in anchor["parents"])
            if is_download_paragraph and urlparse(href).scheme in ("http", "https"):
                return href
        return links.copy_urls[0] if links.copy_urls else None

    if status == 200 and (url := direct_url(body)):
        return {"entry": entry, "status": status, "source": "live", "url": url}
    if saved_slow_html is not None:
        if f"/md5/{md5}" not in saved_slow_html:
            return {"entry": entry, "status": status, "error": "保存的慢速页面与获选记录不符"}
        if url := direct_url(saved_slow_html):
            return {"entry": entry, "status": status, "source": "saved_html", "url": url}
    if "DDoS-Guard" in body or "Checking your browser" in body:
        error = "需要浏览器验证"
    elif status != 200:
        error = "慢速入口不可用"
    else:
        error = "仍在等待或页面未提供直链"
    return {"entry": entry, "status": status, "error": error}


def main():
    parser = argparse.ArgumentParser(
        description=__doc__,
        epilog="会员快速下载密钥可通过 ANNA_SECRET_KEY 提供；无密钥时仍会调用接口并报告错误。",
    )
    parser.add_argument("title", help="书名或搜索词")
    parser.add_argument("--search-html", type=Path, help="浏览器保存的搜索结果 HTML")
    parser.add_argument("--detail-html", type=Path, help="浏览器保存的获选记录详情 HTML")
    parser.add_argument("--slow-html", type=Path, help="浏览器保存的慢速下载页面 HTML")
    parser.add_argument("--base-url", default=BASE, help=argparse.SUPPRESS)
    args = parser.parse_args()
    base = args.base_url.rstrip("/")

    if args.search_html:
        search_html = args.search_html.read_text(encoding="utf-8")
    else:
        search_url = f"{base}/search?{urlencode({'q': args.title, 'ext': 'epub'})}"
        status, _, search_html = get(search_url)
        checked_html(status, search_html, "搜索页")

    records = search_results(search_html)
    if not records:
        raise RuntimeError("未找到 EPUB 结果；请检查搜索词和页面内容")

    counts = {}
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        futures = {pool.submit(metric, base, md5): md5 for md5 in records}
        for future in concurrent.futures.as_completed(futures):
            md5 = futures[future]
            try:
                counts[md5] = future.result()
            except RuntimeError as error:
                raise RuntimeError(f"无法比较全部结果：{md5}: {error}") from error

    winner = max(records, key=lambda md5: (counts[md5], md5))
    detail_html = args.detail_html.read_text(encoding="utf-8") if args.detail_html else None
    saved_slow_html = args.slow_html.read_text(encoding="utf-8") if args.slow_html else None
    try:
        fast = fast_url(base, winner)
    except RuntimeError as error:
        fast = {"error": str(error)}
    try:
        slow = slow_url(base, winner, detail_html, saved_slow_html)
    except RuntimeError as error:
        slow = {"error": str(error)}

    print(json.dumps({
        "title": records[winner],
        "md5": winner,
        "downloads_total": counts[winner],
        "compared": len(records),
        "detail_url": f"{base}/md5/{winner}",
        "fast": fast,
        "slow": slow,
    }, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, OSError) as error:
        print(f"错误：{error}", file=sys.stderr)
        sys.exit(1)
