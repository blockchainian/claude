# ABOUTME: Tests that a translated section title keeps the source's section number form.
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "scripts"))
from restore_verbatim import restore_number  # noqa: E402


def test_number_form_follows_the_source():
    assert restore_number("# 3 Virtual memory applications\n", "# 3. 虚拟内存的应用\n").startswith("# 3 虚拟内存的应用")
    assert restore_number("# 2.1 Traps\n", "# 2.1 陷阱\n").startswith("# 2.1 陷阱")
    assert restore_number("# Acknowledgements\n", "# 致谢\n") == "# 致谢\n"
