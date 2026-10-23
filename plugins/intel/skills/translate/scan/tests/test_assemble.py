# ABOUTME: Tests assemble.py's page joining and front-matter rules on real OCR output of the Appel-Li scan.
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "scripts"))
import assemble  # noqa: E402

PAGE1 = (Path(__file__).parent / "fixtures" / "appel-li-p01.md").read_text()


def front_section():
    sections = assemble.split_sections(assemble.demote_unnumbered(assemble.join_pages([PAGE1])))
    return sections[0]


def test_publisher_notice_is_dropped():
    blocks = assemble.join_pages([PAGE1])
    assert not any("Permission to copy without fee" in b or b.startswith("©") for b in blocks)


def test_byline_goes_to_the_cover_not_the_body():
    title, body = front_section()
    assert title == "Virtual Memory Primitives for User Programs"
    lines, rest = assemble.take_byline(body)
    assert lines == ["Andrew W. Appel and Kai Li", "Department of Computer Science Princeton University"]
    assert rest[0] == "## Abstract"


def test_footnotes_follow_a_rule_at_the_section_end():
    body = ["Para one.", "Note: ¹ A footnote.", "Para two."]
    assert assemble.notes_last(body) == ["Para one.", "Para two.", "---", "¹ A footnote."]


def test_publisher_notice_with_misread_copyright_sign_is_dropped():
    assert assemble.PUBLISHER_RE.match("-c 2012 ACM 978-1-4503-1234-5")


def test_continuation_skips_a_float_on_its_own_page():
    prev = "First paragraph that runs on to the next"
    page2 = "| a | b |\n|---|---|\n| 1 | 2 |\n\n**Table 1: Data.**\n\nLegend under the table.\n\n<<CONT>>\npage, where it ends."
    blocks = assemble.join_pages([prev, page2])
    assert blocks[0] == "First paragraph that runs on to the next page, where it ends."
    assert blocks[-1] == "Legend under the table."


def test_printed_urls_become_short_links():
    blocks = assemble.join_pages(["See http://www.cs.princeton.edu/~appel/papers/vmpup.ps for the code."])
    assert blocks == ["See [www.cs.princeton.edu](http://www.cs.princeton.edu/~appel/papers/vmpup.ps) for the code."]


def test_a_link_labelled_with_its_own_address_gets_the_host_name():
    blocks = assemble.join_pages(["visit: [http://ocw.mit.edu/terms](http://ocw.mit.edu/terms)."])
    assert blocks == ["visit: [ocw.mit.edu](http://ocw.mit.edu/terms)."]


def test_the_opencourseware_terms_page_is_dropped():
    ocw = (Path(__file__).parent / "fixtures" / "ocw-terms-page.md").read_text()
    assert assemble.join_pages(["Last line of the notes.", ocw]) == ["Last line of the notes."]


def test_a_sketch_crop_keeps_its_labels_and_leaves_the_text_around_it():
    """6.1220 L03 page 2: codex boxed the convolution sketch loosely in one run (taking in the next line) and
    tightly in another (cutting off the "shifts of rev(B)" label); both must crop the whole sketch only."""
    from PIL import Image
    fx = Path(__file__).parent / "fixtures"
    im = Image.open(fx / "handwritten-6.1220-L03-p02.png")
    W, H = im.size
    md_lines = (fx / "handwritten-6.1220-L03-p02.md").read_text().splitlines()
    at = next(i for i, l in enumerate(md_lines) if l.startswith("[FIGURE"))
    lines = [(*map(float, b.split()), t) for b, t in
             (l.split("\t", 1) for l in (fx / "handwritten-6.1220-L03-p02.vision.txt").read_text().splitlines())]
    for box in ([0.226, 0.123, 0.872, 0.194], [0.226, 0.122, 0.976, 0.263]):
        x0, y0, x1, y1 = assemble.figure_box(im, box, lines, assemble.neighbour_text(md_lines, at))
        assert x1 >= 0.965 * W, box  # "rev(B)" ends at 0.969
        assert y0 >= 0.1164 * H - 2, box  # below "inner product of all relative shifts"
        assert y1 <= 0.1964 * H + 2, box  # above "useful for smoothing etc."


def test_a_later_page_title_keeps_its_section_when_ocr_uses_a_top_level_heading():
    fixtures = Path(__file__).parent / "fixtures"
    pages = [(fixtures / f"literate-programming-p{n:02}.md").read_text() for n in (1, 2)]
    sections = assemble.split_sections(assemble.demote_unnumbered(assemble.join_pages(pages)))
    assert [title for title, _ in sections] == [
        "Chapter 1", "Introduction by B. A. Galler", "Computer Programming as an Art, by D. E. Knuth"
    ]
