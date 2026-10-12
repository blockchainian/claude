# ABOUTME: Text rules shared by the paper pipelines (typeset and scanned): publisher boilerplate and the OCW terms
# ABOUTME: page to drop, and printed URLs turned into Markdown links with short labels.
import re

# Publisher boilerplate dropped from the translation (the rule shared with the scan pipeline); "-c" is a © misread.
PUBLISHER_RE = re.compile(r"^(Note:\s*)?(Permission to (make digital|copy without fee)|©|(\(c\)|-c)\s*\d{4}|Copyright\b.*\d{4}"
                          r"|This work is licensed under a Creative Commons|Designations used by companies to distinguish"
                          r"|\**Suggestions, Comments, Corrections)", re.I)

# The MIT OpenCourseWare terms page appended to OCW PDFs: dropped whole (the rule shared with the scan pipeline).
OCW_PAGE_RE = re.compile(r"MIT OpenCourseWare[\s\S]*ocw\.mit\.edu/terms")

DOI_URL = re.compile(r"\bDOI\s*[=:]?\s*(10\.\d{4,}/\S+?)[.,;]?\s+(https?://\S+?)(?=[.,;]?(?:\s|$))", re.I)
BARE_URL = re.compile(r"(?<![(\[<])\b(https?://[^\s<>()\[\]]+?)(?=[.,;:]?(?:\s|$|\)))")


def link_label(label, url):
    """The visible text of a link: the source's own text unless it is the URL itself, then the host name."""
    if label.strip().rstrip("/") not in (url.rstrip("/"), url.split("://", 1)[-1].rstrip("/")):
        return label.strip()
    return url.split("://", 1)[-1].split("/", 1)[0]


def linkify(text):
    """A printed URL becomes a Markdown link, so the page shows a short label instead of the address: "DOI =
    10.1145/x http://doi.acm.org/10.1145/x" -> "DOI: [10.1145/x](http://…)", any other URL -> [host](url)."""
    text = DOI_URL.sub(lambda m: f"DOI: [{m.group(1)}]({m.group(2)})", text)
    return BARE_URL.sub(lambda m: f"[{link_label(m.group(1), m.group(1))}]({m.group(1)})", text)


def drop_publisher(text):
    """The text without its publisher-boilerplate lines (permission notice, copyright line)."""
    return "\n".join(line for line in text.split("\n") if not PUBLISHER_RE.match(line.strip())).strip()
