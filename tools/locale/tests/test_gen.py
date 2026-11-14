from __future__ import annotations

import io
import json
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

import gen
from tests.helpers import Workdir


class Run:
    def __init__(self, work: Workdir):
        self.work = work
        self.rust = work.path / "out/generated.rs"
        self.arb = work.path / "out/l10n"

    def args(self, *extra: str):
        return ["--locales", str(self.work.locales), "--rust", str(self.rust), "--arb-dir", str(self.arb), *extra]

    def run(self, *extra: str):
        out, err = io.StringIO(), io.StringIO()
        with redirect_stdout(out), redirect_stderr(err):
            code = gen.main(self.args(*extra))
        return code, out.getvalue(), err.getvalue()


class GenTests(unittest.TestCase):
    def setUp(self):
        self.work = Workdir()
        self.work.copy_tree()
        self.run_ = Run(self.work)

    def tearDown(self):
        self.work.cleanup()

    def test_writes_then_check_passes(self):
        code, out, err = self.run_.run()
        self.assertEqual((code, err), (0, ""))
        self.assertIn("wrote", out)
        self.assertTrue(self.run_.rust.is_file())
        self.assertEqual(sorted(p.name for p in self.run_.arb.iterdir()), ["app_en.arb", "app_pt.arb", "app_pt_BR.arb"])
        code, out, err = self.run_.run("--check")
        self.assertEqual((code, out, err), (0, "", ""))

    def test_second_run_writes_nothing(self):
        self.run_.run()
        code, out, err = self.run_.run()
        self.assertNotIn("wrote", out)

    def test_check_fails_on_stale_rust(self):
        self.run_.run()
        path = self.work.locales / "en/mac/chat.ftl"
        path.write_text(path.read_text(encoding="utf-8").replace("= Move\n", "= Relocate\n"), encoding="utf-8")
        code, out, err = self.run_.run("--check")
        self.assertEqual(code, 1)
        self.assertIn("GUARD-LOCALE-GEN", err)
        self.assertIn("generated.rs", err)
        self.assertIn("run python3 tools/locale/gen.py", err)

    def test_check_fails_on_hand_edit(self):
        self.run_.run()
        with open(self.run_.rust, "a", encoding="utf-8") as handle:
            handle.write(" ")
        code, out, err = self.run_.run("--check")
        self.assertEqual(code, 1)
        self.assertIn("GUARD-LOCALE-GEN", err)

    def test_check_fails_on_missing_output(self):
        code, out, err = self.run_.run("--check")
        self.assertEqual(code, 1)
        self.assertIn("generated.rs", err)

    def test_arb_written_only_when_output_on(self):
        self.work.config(ship=("en", "pt", "pt-BR"), draft=("de",), outputs=("rust",))
        code, _, err = self.run_.run()
        self.assertEqual(code, 0, err)
        self.assertFalse(self.run_.arb.exists())
        self.run_.arb.mkdir(parents=True)
        (self.run_.arb / "app_en.arb").write_text("{}", encoding="utf-8")
        code, _, err = self.run_.run("--check")
        self.assertEqual((code, err), (0, ""))
        self.assertEqual((self.run_.arb / "app_en.arb").read_text(encoding="utf-8"), "{}")

    def test_stale_arb_file_is_reported_and_removed(self):
        self.run_.run()
        stale = self.run_.arb / "app_de.arb"
        stale.write_text("{}", encoding="utf-8")
        code, _, err = self.run_.run("--check")
        self.assertEqual(code, 1)
        self.assertIn("app_de.arb", err)
        self.run_.run()
        self.assertFalse(stale.exists())

    def test_check_reports_every_problem_then_exits_1(self):
        (self.work.locales / "en/phone/pairing.ftl").write_text("pairing-x = a \n", encoding="utf-8")
        (self.work.locales / "en/mac/units.ftl").write_text("units-x = b {\n", encoding="utf-8")
        code, out, err = self.run_.run("--check")
        self.assertEqual(code, 1)
        lines = [l for l in err.splitlines() if l]
        self.assertEqual(len(lines), 2, err)
        self.assertTrue(all(l.startswith("GUARD-LOCALE-SUBSET: ") for l in lines))

    def test_new_copies_and_lists_draft(self):
        code, out, err = self.run_.run("--new", "fr")
        self.assertEqual((code, err), (0, ""))
        self.assertTrue((self.work.locales / "fr/mac/chat.ftl").is_file())
        shared = (self.work.locales / "fr/shared.ftl").read_text(encoding="utf-8")
        self.assertNotIn("-brand-", shared.split("language-name")[0].replace("{ -brand-", ""))
        cfg = json.loads((self.work.locales / "languages.json").read_text(encoding="utf-8"))
        self.assertEqual(cfg["draft"], ["de", "fr"])
        self.assertEqual(cfg["ship"], ["en", "pt", "pt-BR"])
        code, out, err = self.run_.run("--check")
        self.assertNotIn("GUARD-LOCALE-SUBSET", err)

    def test_new_refuses_existing_and_bad_tag(self):
        code, _, err = self.run_.run("--new", "de")
        self.assertEqual(code, 1)
        self.assertIn("exists already", err)
        code, _, err = self.run_.run("--new", "fr-ca")
        self.assertEqual(code, 1)
        self.assertIn("not a language tag", err)
        self.assertFalse((self.work.locales / "fr-ca").exists())

    def test_prune_removes_unknown_keys_only(self):
        path = self.work.locales / "de/mac/chat.ftl"
        path.write_text(
            path.read_text(encoding="utf-8")
            + "\n# an old note\nchat-gone = Weg\n\nchat-also-gone = { $count ->\n    [one] a\n   *[other] b\n}\n",
            encoding="utf-8",
        )
        code, out, err = self.run_.run("--prune")
        self.assertEqual((code, err), (0, ""))
        text = path.read_text(encoding="utf-8")
        self.assertNotIn("gone", text)
        self.assertNotIn("old note", text)
        self.assertIn("chat-moved-to = Verschoben nach { $folder_name }", text)
        self.assertIn("chat-move = Verschieben", text)
        self.assertTrue(text.endswith("Verschieben\n"))
        code, out, err = self.run_.run("--check")
        self.assertNotIn("GUARD-LOCALE-MATCH", err)

    def test_ios_and_android_flags_write_files(self):
        ios = self.work.path / "ios"
        android = self.work.path / "android"
        code, out, err = self.run_.run("--ios", str(ios), "--android", str(android))
        self.assertEqual((code, err), (0, ""))
        self.assertTrue((ios / "Localizable.xcstrings").is_file())
        self.assertTrue((android / "values/strings.xml").is_file())
        self.assertTrue((android / "values-pt-rBR/strings.xml").is_file())

    def test_missing_messages_never_fail_check(self):
        self.run_.run()
        (self.work.locales / "de/mac/chat.ftl").unlink()
        code, out, err = self.run_.run("--check")
        self.assertEqual((code, err), (0, ""))

    def test_report_flag_prints_table(self):
        code, out, err = self.run_.run("--report")
        self.assertEqual(code, 0)
        self.assertTrue(out.startswith("Language"))
        code, out, err = self.run_.run("--report", "--lang", "de", "--markdown")
        self.assertIn("| de |", out)
        self.assertNotIn("| pt |", out)
        code, out, err = self.run_.run("--report", "--lang", "de", "--missing")
        self.assertIn("de: units-age-minutes", out)
        self.assertNotIn("chat-move", out)


if __name__ == "__main__":
    unittest.main()
