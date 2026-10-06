# ABOUTME: Identifies protected Markdown code regions before math and prose processing.
import re

CODE_CONTENT_RE = re.compile(
    r"^(`{3,}|~{3,})[^\n]*\n.*?^\1[ \t]*$|<pre\b[^>]*>.*?</pre>|(`+)[^\n]*?\2",
    re.M | re.S,
)
CODE_TOKEN_RE = re.compile(r"⟦CODE:([^⟧]+)⟧")
