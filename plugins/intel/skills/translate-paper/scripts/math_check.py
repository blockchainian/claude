#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["markdown>=3.5", "pikepdf>=9", "pillow>=10"]
# ///
# ABOUTME: Lists the formulas in a work dir's translated Markdown that render.py would print as red KaTeX errors:
# ABOUTME: runs render.py's own Markdown and math repair, then KaTeX with the renderer's settings.
#
# Usage: math_check.py <work dir> [id ...]   (exit 1 when any formula fails)
import html
import importlib.util
import json
import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("render", HERE.parents[1] / "translate" / "scripts" / "render.py")
render = importlib.util.module_from_spec(spec)
spec.loader.exec_module(render)

KATEX = """
const k = require("katex");
const items = JSON.parse(require("fs").readFileSync(0, "utf8"));
for (const [id, m] of items) {
  const out = k.renderToString(m.slice(2, -2), { displayMode: m.startsWith("\\\\["), throwOnError: false });
  if (/katex-error|#cc0000/i.test(out)) console.log(id + "\\t" + m.replace(/\\s+/g, " ").slice(0, 200));
}
"""


def main():
    work = Path(sys.argv[1]).resolve()
    only = set(sys.argv[2:])
    codes_path = work / "codeblocks.json"
    codes = json.loads(codes_path.read_text()) if codes_path.exists() else {}
    items = []
    for md in sorted((work / "translated").glob("*.md")):
        if md.name.endswith(".rejected.md") or (only and md.stem not in only):
            continue
        _, body = render.md_to_html(md.read_text(), codes)
        for m in re.findall(r"\\\[.*?\\\]|\\\(.*?\\\)", html.unescape(body), re.S):
            items.append((md.stem, m))
    out = subprocess.run(["node", "-e", KATEX], input=json.dumps(items), capture_output=True, text=True, cwd=HERE, check=True).stdout
    print(out, end="")
    print(f"{len(items)} formulas, {out.count(chr(10))} would render red")
    sys.exit(1 if out else 0)


if __name__ == "__main__":
    main()
