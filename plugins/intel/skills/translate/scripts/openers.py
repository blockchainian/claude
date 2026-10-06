# ABOUTME: Finds top-level opening paragraphs and recognizes figure-caption labels.
import re
from html.parser import HTMLParser

CAPTION_TEXT_RE = re.compile(r"^\s*(?:[图表](?=\s|\d|[：:。.])|Fig\.?\s|Table\s)", re.I)


class Paragraphs(HTMLParser):
    def __init__(self, body):
        super().__init__(convert_charrefs=False)
        self.body = body
        self.lines = [0]
        for m in re.finditer("\n", body):
            self.lines.append(m.end())
        self.stack = []
        self.items = []
        self.current = None
        self.feed(body)

    def absolute_position(self):
        line, column = self.getpos()
        return self.lines[line - 1] + column

    def handle_starttag(self, tag, attrs):
        if tag == "p" and not self.stack:
            self.current = {"start": self.absolute_position(), "inner": self.absolute_position() + len(self.get_starttag_text()), "attrs": dict(attrs)}
        if tag not in {"img", "br", "hr", "wbr", "input", "meta", "link", "source"}:
            self.stack.append(tag)

    def handle_endtag(self, tag):
        if tag == "p" and self.current is not None and self.stack == ["p"]:
            self.current["head"] = self.body[self.current["inner"]:self.absolute_position()]
            self.items.append(self.current)
            self.current = None
        if tag in self.stack:
            del self.stack[len(self.stack) - 1 - self.stack[::-1].index(tag):]

    def handle_startendtag(self, tag, attrs):
        pass


def opening_paragraph(body):
    for p in Paragraphs(body).items:
        text = re.sub(r"<[^>]+>", "", p["head"])
        if "caption" in (p["attrs"].get("class") or "").lower() or CAPTION_TEXT_RE.match(text):
            continue
        return p
    return None
