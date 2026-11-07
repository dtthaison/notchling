import io
import os
import unittest
from contextlib import redirect_stdout
from pathlib import Path

from guard.__main__ import main
from guard.codeowners import render
from guard.zones import load
from tests.helpers import make_repo

ROOT = Path(__file__).resolve().parents[3]
Z = load(ROOT / "tools/guard/zones.json")


class CodeownersTests(unittest.TestCase):
    def test_codeowners_starts_with_owner(self):
        lines = [l for l in render(Z).splitlines() if l and not l.startswith("#")]
        self.assertEqual(lines[0], "* @dtthaison")

    def test_zone_lines_have_no_owner(self):
        lines = [l for l in render(Z).splitlines() if l.startswith("/Sources/Notchling/Chat/Views/")]
        self.assertEqual(lines, ["/Sources/Notchling/Chat/Views/"])

    def test_exclusions_reowned_after_zone(self):
        lines = render(Z).splitlines()
        self.assertGreater(lines.index("/docs/rules.md @dtthaison"), lines.index("/docs/"))

    def test_real_file_up_to_date(self):
        self.assertEqual((ROOT / ".github/CODEOWNERS").read_text(), render(Z))

    def test_codeowners_check_fails_on_stale(self):
        repo = make_repo({"tools/guard/zones.json": (ROOT / "tools/guard/zones.json").read_text(),
                          ".github/CODEOWNERS": "* @someone\n"})
        cwd = os.getcwd()
        try:
            os.chdir(repo)
            out = io.StringIO()
            with redirect_stdout(out):
                code = main(["codeowners", "--check"])
        finally:
            os.chdir(cwd)
        self.assertEqual(code, 1)
        self.assertIn("GUARD-CODEOWNERS", out.getvalue())


if __name__ == "__main__":
    unittest.main()
