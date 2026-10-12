#!/usr/bin/env -S uv run --quiet --script
# /// script
# dependencies = ["pymupdf", "pikepdf", "pillow"]
# ///
# ABOUTME: Contact sheet of a PDF's pages (optionally a page range) as one PNG, page numbers in red. With --apple the
# pages are rendered by macOS's own PDF engine (sips), which Preview uses and which differs from MuPDF on details
# such as JPEG 2000 transparency.
# Usage: sheet.py <pdf> <out.png> [--cols 4] [--width 320] [--pages 1-8] [--apple]
import os, subprocess, sys, tempfile
import pikepdf, pymupdf
from PIL import Image, ImageDraw

args = sys.argv[1:]
apple = "--apple" in args
args = [a for a in args if a != "--apple"]
opt = {"--cols": "4", "--width": "320", "--pages": None}
for k in list(opt):
    if k in args:
        i = args.index(k); opt[k] = args[i + 1]; del args[i:i + 2]
src, out = args
d = pymupdf.open(src)
a, b = (int(x) for x in opt["--pages"].split("-")) if opt["--pages"] else (1, len(d))
pages = list(range(a - 1, b))
cols, W = int(opt["--cols"]), int(opt["--width"])
H = round(W * d[0].rect.height / d[0].rect.width)


def render(i, tmp):
    if not apple:
        z = W / d[i].rect.width
        pix = d[i].get_pixmap(matrix=pymupdf.Matrix(z, z))
        return Image.frombytes("RGB", (pix.width, pix.height), pix.samples)
    one, png = os.path.join(tmp, f"{i}.pdf"), os.path.join(tmp, f"{i}.png")
    pdf = pikepdf.open(src); single = pikepdf.new(); single.pages.append(pdf.pages[i]); single.save(one)
    subprocess.run(["sips", "-s", "format", "png", one, "--out", png], check=True, capture_output=True)
    return Image.open(png).convert("RGB")


sheet = Image.new("RGB", (W * cols, H * ((len(pages) + cols - 1) // cols)), "white")
draw = ImageDraw.Draw(sheet)
with tempfile.TemporaryDirectory() as tmp:
    for n, i in enumerate(pages):
        x, y = (n % cols) * W, (n // cols) * H
        sheet.paste(render(i, tmp).resize((W, H)), (x, y))
        draw.text((x + 3, y + 2), str(i + 1), fill="red")
sheet.save(out)
