#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["pymupdf>=1.24"]
# ///
# ABOUTME: Tests pdf2work.py's Markdown building on MinerU span and block shapes taken from real middle_json output:
# ABOUTME: math tidying, footnote marks vs exponents and primes, links, boilerplate, word counts, splitting, grids.
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "scripts"))
import pdf2work as p  # noqa: E402
from paper_rules import drop_publisher, linkify  # noqa: E402

fails = []


def check(name, cond, detail=""):
    print(f"{'PASS' if cond else 'FAIL'}: {name}" + (f"  -- {detail}" if not cond and detail else ""))
    if not cond:
        fails.append(name)


# tidy_math: sentence punctuation leaves the formula, but "\right." (an invisible delimiter) stays whole
check("trailing comma moves out", p.tidy_math(r"s \times s ,") == (r"s \times s", ","))
got = p.tidy_math(r"\left\{ \begin{array}{l} a \end{array} \right.")
check("\\right. kept", got == (r"\left\{ \begin{array}{l} a \end{array} \right.", ""), str(got))
check("thin space kept", p.tidy_math(r"a \,") == (r"a \,", ""))
check("\\textgreater becomes >", p.tidy_math(r"w [ S ] \textgreater w [ T ]")[0] == "w [ S ] > w [ T ]")
got = p.tidy_math(r"\begin{array}{c} \text {Since by the above lemma A(t) = \hat{A} (t), we have A(t) =} \\ O (U (t)) \end{array}")[0]
check("math command lifted out of \\text", r"\text {Since by the above lemma A(t) = }\hat{A}\text{ (t), we have A(t) =}" in got, got)
check("subscript lifted out of \\text", p.tidy_math(r"f = 1 \text {if T_{k} holds}")[0] == r"f = 1 \text {if }T_{k}\text{ holds}")
check("stray $ inside a formula dropped", p.tidy_math(r"(i.e., $\mathtt { o } = \mathtt { p }$)")[0] == r"(i.e., \mathtt { o } = \mathtt { p })")
real = r"\begin{array}{l} \text {if (A[i] > A[i + 1]) \{} \\ \quad \dots \\ \text {} \end{array}"
check("escaped brace inside \\text", p.tidy_math(real)[0] == real, p.tidy_math(real)[0])
check("chained scripts lifted out of \\text", p.tidy_math(r"\text {for rchpk_{0}^{A} [, }")[0] == r"\text {for }rchpk_{0}^{A}\text{ [, }")
check("unclosed group closed", p.tidy_math(r"\mathbf { \hat { A d v } _ { 3 }")[0] == r"\mathbf { \hat { A d v } _ { 3 }}")
check("plain \\text untouched", p.tidy_math(r"x \text {if} y")[0] == r"x \text {if} y")
check("\\textellipsis becomes \\ldots", p.tidy_math(r"{ \textellipsis } A k")[0] == r"{ \ldots } A k")
check("\\ref dropped", p.tidy_math(r"\text {Bob says} M _ {1} \quad (\ref {e q : 1})")[0] == r"\text {Bob says} M _ {1} \quad")
check("\\textmu becomes \\mu", "\\mu" in p.tidy_math(r"K ^ { \textmu }")[0])
check("hyphenated upright name becomes \\text", p.tidy_math(r"\mathrm{ORD-MULT}(A, B)")[0] == r"\text{ORD-MULT}(A, B)")

# spans_md: the document's footnote numbers stay <sup>; any other raised digit is an exponent or a prime
p.FOOTNOTE_MARKS.clear()
p.FOOTNOTE_MARKS.add("1")
sup = lambda c: {"type": "text", "content": c, "styles": ["superscript"]}
txt = lambda c: {"type": "text", "content": c}
check("footnote mark", p.spans_md([txt("optimal"), sup("1"), txt(" algorithms")]) == "optimal<sup>1</sup> algorithms")
check("exponent", p.spans_md([txt("Θ(n"), sup("2"), txt(")")]) == "Θ(n²)")
check("name with raised digit is not a footnote", p.spans_md([txt("the L"), sup("1"), txt(" kernel")]) == "the L¹ kernel")
check("name with a spaced raised digit", p.spans_md([txt("user-mode L "), sup("1"), txt(" Linux")]) == "user-mode L¹ Linux")
check("name with a raised digit set as math", p.spans_md([txt("user-mode L "), {"type": "equation_inline", "content": "^ { 4 }"}, txt(" Linux")]) == "user-mode L⁴ Linux")
check("prime from superscript 0", p.spans_md([txt("first k"), sup("0"), txt(" contains")]) == "first k′ contains")
check("prime from math", p.spans_md([txt("k"), {"type": "equation_inline", "content": "^{0}"}]) == "k′")
check("footnote mark set as math", p.spans_md([txt("analyzer,"), {"type": "equation_inline", "content": "^{1}"}]) == "analyzer,<sup>1</sup>")
check("citation read as math", p.spans_md([txt("model "), {"type": "equation_inline", "content": "[5,20]"}, txt(" , which")])
      == "model [5, 20], which")
check("inline math", p.spans_md([txt("multiply two "), {"type": "equation_inline", "content": "n \\times n"}, txt(" matrices")])
      == r"multiply two \(n \times n\) matrices")
link = {"type": "hyperlink", "url": "http://ocw.mit.edu/terms", "content": [txt("http://ocw.mit.edu/terms")]}
check("hyperlink span labelled by host", p.spans_md([txt("visit: "), link]) == "visit: [ocw.mit.edu](http://ocw.mit.edu/terms)")

check("ordinal suffix read as math", p.spans_md([txt("the 99"), {"type": "equation_inline", "content": "^{th}"}, txt(" percentile")])
      == "the 99th percentile")
check("number with ordinal read as math", p.spans_md([txt("the "), {"type": "equation_inline", "content": "9 9 . \\dot { 9 } ^ { t h }"}, txt(" percentile")])
      == "the 99.9th percentile")
check("quoted character read as math", p.spans_md([txt("reads "), {"type": "equation_inline", "content": "^ { \\circ } 0 ^ { \\prime }"}, txt(" back")]) == "reads '0' back")
p.FOOTNOTE_MARKS.add("1")
check("phrase read as math", p.spans_md([txt("site "), {"type": "equation_inline", "content": "( { \\it e . g . , \\; \\tt b a d . c o m } ^ { 1 } )"}, txt(".")])
      == "site (e.g., bad.com<sup>1</sup>).")
check("real math is not a phrase", p.prose_as_math(r"( a , b )") is None and p.prose_as_math(r"O ( n \log n )") is None)
check("unraised footnote number before a word", p.note_mark("2 To avoid worrying about small constants") == "<sup>2</sup>To avoid worrying about small constants")
check("unraised footnote number", p.note_mark("3[smile.amazon.com](http://smile.amazon.com/s)") == "<sup>3</sup>[smile.amazon.com](http://smile.amazon.com/s)")
check("section sign read as math", p.spans_md([txt("see "), {"type": "equation_inline", "content": "\\S 4"}, txt(".")]) == "see §4.")
check("percentage read as math", p.spans_md([txt("in "), {"type": "equation_inline", "content": "5\\%"}, txt(" increments")])
      == "in 5% increments")

marks = p.footnote_marks({"pages": [{"page_idx": 0, "blocks": [{"type": "page_footnote", "bbox": [0.1, 0.82, 0.48, 0.89], "content": [
    sup("5"), txt("As mentioned above under Linux this invariant can be violated."), sup("6"), txt("This approach also requires a modified loader.")]}]}]})
check("every footnote in a merged footnote block", marks == {"5", "6"}, str(marks))
check("NUL bullet glyph removed", p.spans_md([txt("- \x00 Copy arguments")]) == "- Copy arguments")

# links and boilerplate
check("DOI line", linkify("DOI = 10.1145/2071379.2071383 http://doi.acm.org/10.1145/2071379.2071383")
      == "DOI: [10.1145/2071379.2071383](http://doi.acm.org/10.1145/2071379.2071383)")
check("bare URL in parentheses", linkify("(available at http://software.intel.com/en-us/articles/intel-cilk/) differs")
      == "(available at [software.intel.com](http://software.intel.com/en-us/articles/intel-cilk/)) differs")
check("copyright line dropped, venue kept",
      drop_publisher("SPAA'10, June 13–15, 2010, Thira, Santorini, Greece.\nCopyright 2010 ACM 978-1-4503-0079-7/10/06 ...$10.00.")
      == "SPAA'10, June 13–15, 2010, Thira, Santorini, Greece.")
check("misread © dropped", drop_publisher("-c 2012 ACM 1549-6325/2012/01-ART4 $10.00") == "")
check("printed contents line", bool(p.DOT_LEADER.search("8.1 Faults, Failures, and Fault Tolerant Design................................8–3")))

toc = "Contents\n" + "\n".join(f"1.{i} Section {i} . . . . . . . . . . . .\n{i + 10}" for i in range(8))
check("printed contents page", p.contents_page(toc))
check("a prose page is not contents", not p.contents_page("\n".join(["A line of prose about kernels."] * 12)))

# words: the translator's length floor counts prose only, not math, tokens or table rows
check("words skip math and tables", p.words("a word \\(x + y\\)\n| BEQ(Ra, label) | BEQ(Ra, label, R31) |\nmore words ⟦IMG:p1-2⟧") == 3)

# split_large: a long section splits at its headings, each later piece titled by its heading
para = lambda n: ("p", " ".join(["word"] * n), "")
sec = {"title": "Chapter", "items": [para(3000), ("h", 2, "8.2 Next", ""), para(3000), ("h", 2, "8.3 Last", ""), para(500)]}
pieces = p.split_large([sec], 4000)
check("split at headings", [s["title"] for s in pieces] == ["Chapter", "8.2 Next"], str([s["title"] for s in pieces]))

dense = {"title": "Prompts", "items": [it for k in range(10) for it in [("h", 2, f"Turn {k}", "")] +
                                       [("raw", "⟦CODE:c%d⟧" % (20 * k + i), "") for i in range(20)]]}
check("a placeholder-dense section is split", len(p.split_large([dense], 4000)) >= 2)

# figure_groups: six sub-figures with "(a)".."(f)" labels and one "Figure 14:" caption become one grid
def image(bbox, *caps):
    return {"type": "image", "bbox": bbox, "content": [{"type": "image_body", "bbox": bbox}] +
            [{"type": "image_caption", "bbox": [bbox[0], bbox[3], bbox[2], bbox[3] + 0.02], "content": [txt(c)]} for c in caps]}

grid = [image([0.177, 0.066, 0.451, 0.317], "(a) Bzip2."), image([0.545, 0.068, 0.821, 0.317], "(b) Morphy."),
        image([0.177, 0.629, 0.451, 0.879], "(e) Min poset."),
        image([0.545, 0.628, 0.821, 0.879], "(f) Bivariate polynomial multiplication.", "Figure 14: Cilkview graphical output."),
        {"type": "page_number", "bbox": [0.488, 0.95, 0.512, 0.962], "content": [txt("154")]}]
groups = p.figure_groups(grid)
check("sub-figure grid grouped", len(groups.get(0, [])) == 4 and groups.get(1) == [] and 4 not in groups, str(groups.keys()))
separate = [image([0.1, 0.1, 0.4, 0.3], "Fig. 3. One."), image([0.5, 0.1, 0.9, 0.3], "Fig. 4. Two.")]
check("two captioned figures stay apart", p.figure_groups(separate) == {})

print(f"\n{len(fails)} failed" if fails else "\nall passed")
sys.exit(1 if fails else 0)
