import base64
import contextlib
import datetime
import email.utils
import io
import locale
import os
import re
import tempfile
import unittest
import xml.etree.ElementTree as ET
from unittest import mock

import appcast

SPARKLE = "http://www.andymatuschak.org/xml-namespaces/sparkle"
GOLDEN = os.path.join(os.path.dirname(__file__), "golden")

FIXED_NOW = datetime.datetime(2026, 11, 8, 9, 5, 3, tzinfo=datetime.timezone.utc)
FIXED_NOW_TEXT = "Sun, 08 Nov 2026 09:05:03 +0000"
EXACT_DATE = re.compile(r"^(Mon|Tue|Wed|Thu|Fri|Sat|Sun), \d\d (Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec) "
                        r"\d{4} \d\d:\d\d:\d\d \+0000$")

NOT_EXACT_DATES = {
    "unpadded day": "Sun, 8 Nov 2026 10:00:00 +0000",
    "GMT zone": "Sun, 08 Nov 2026 10:00:00 GMT",
    "other offset": "Sun, 08 Nov 2026 10:00:00 -0800",
    "positive offset": "Sun, 08 Nov 2026 10:00:00 +0100",
    "no weekday": "08 Nov 2026 10:00:00 +0000",
    "no seconds": "Sun, 08 Nov 2026 10:00 +0000",
    "two digit year": "Sun, 08 Nov 26 10:00:00 +0000",
    "wrong weekday": "Mon, 08 Nov 2026 10:00:00 +0000",
    "lower case": "sun, 08 nov 2026 10:00:00 +0000",
    "long names": "Sunday, 08 November 2026 10:00:00 +0000",
    "trailing space": "Sun, 08 Nov 2026 10:00:00 +0000 ",
    "trailing newline": "Sun, 08 Nov 2026 10:00:00 +0000\n",
    "no such day": "Sun, 31 Nov 2026 10:00:00 +0000",
    "hour 25": "Sun, 08 Nov 2026 25:00:00 +0000",
    "iso": "2026-11-08T10:00:00Z",
    "now with a space": "now ",
    "capital now": "Now",
    "empty": "",
    "words": "yesterday",
}

SIG_A = base64.b64encode(bytes(range(64))).decode()
SIG_B = base64.b64encode(bytes(range(64, 128))).decode()
SIG_C = base64.b64encode(bytes(range(128, 192))).decode()


def q(name):
    return "{%s}%s" % (SPARKLE, name)


def canon(element):
    element.tail = None
    return ET.tostring(element)


def add_args(feed, version, short, sig=SIG_A, channel=None, extra=()):
    args = [
        "add", "--feed", feed, "--version", str(version), "--short", short,
        "--url", "https://github.com/dtthaison/notchling/releases/download/v%s/Notchling-%s.dmg" % (short, short),
        "--length", "1234567", "--ed-signature", sig, "--min-system", "13.0",
        "--notes-url", "https://github.com/dtthaison/notchling/releases/tag/v%s" % short,
    ]
    if channel:
        args += ["--channel", channel]
    return args + list(extra)


class CliCase(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.feed = os.path.join(self.tmp.name, "appcast.xml")

    def run_cli(self, args):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = appcast.main(args)
        return code, out.getvalue(), err.getvalue()

    def read(self):
        with open(self.feed, encoding="utf-8") as f:
            return f.read()

    def items(self):
        return ET.parse(self.feed).getroot().find("channel").findall("item")

    def versions(self):
        return [i.findtext(q("version")) for i in self.items()]

    def golden(self, name):
        with open(os.path.join(GOLDEN, name), encoding="utf-8") as f:
            return f.read()

    def build_sample(self):
        self.assertEqual(self.run_cli(add_args(self.feed, 12, "1.2.0"))[0], 0)
        self.assertEqual(self.run_cli(add_args(self.feed, 15, "1.3.0-beta.1", SIG_B, "beta"))[0], 0)
        self.assertEqual(self.run_cli(add_args(self.feed, 14, "1.2.1", SIG_C))[0], 0)
        extra = ["--rollout", "3600", "--critical",
                 "--pub-date", "Sun, 08 Nov 2026 10:00:00 +0000"]
        self.assertEqual(self.run_cli(add_args(self.feed, 16, "1.3.0-beta.2", SIG_A, "beta", extra))[0], 0)


class TestAdd(CliCase):
    def test_creates_feed_with_title_and_link(self):
        code, _, _ = self.run_cli(add_args(self.feed, 7, "1.0.0"))
        self.assertEqual(code, 0)
        channel = ET.parse(self.feed).getroot().find("channel")
        self.assertEqual(channel.findtext("title"), "Notchling")
        self.assertEqual(channel.findtext("link"), "https://github.com/dtthaison/notchling")

    def test_stable_item_fields(self):
        self.run_cli(add_args(self.feed, 7, "1.0.0"))
        (item,) = self.items()
        self.assertEqual(item.findtext(q("version")), "7")
        self.assertEqual(item.findtext(q("shortVersionString")), "1.0.0")
        self.assertEqual(item.findtext(q("minimumSystemVersion")), "13.0")
        self.assertEqual(item.findtext(q("phasedRolloutInterval")), "86400")
        self.assertEqual(item.findtext(q("releaseNotesLink")),
                         "https://github.com/dtthaison/notchling/releases/tag/v1.0.0")
        self.assertIsNone(item.find(q("channel")))
        self.assertIsNone(item.find(q("criticalUpdate")))
        self.assertIsNone(item.find("pubDate"))

    def test_enclosure_attributes(self):
        self.run_cli(add_args(self.feed, 7, "1.0.0"))
        enc = self.items()[0].find("enclosure")
        self.assertEqual(enc.get("url"),
                         "https://github.com/dtthaison/notchling/releases/download/v1.0.0/Notchling-1.0.0.dmg")
        self.assertEqual(enc.get("length"), "1234567")
        self.assertEqual(enc.get("type"), "application/octet-stream")
        self.assertEqual(enc.get(q("edSignature")), SIG_A)

    def test_sparkle_two_placement(self):
        self.run_cli(add_args(self.feed, 9, "1.0.0-beta.1", SIG_A, "beta", ["--critical"]))
        item = self.items()[0]
        for name in ("version", "shortVersionString", "channel", "minimumSystemVersion",
                     "phasedRolloutInterval", "criticalUpdate", "releaseNotesLink"):
            self.assertIsNotNone(item.find(q(name)), name)
        enc = item.find("enclosure")
        self.assertEqual(sorted(enc.attrib), sorted(["url", "length", "type", q("edSignature")]))
        self.assertEqual(item.findtext(q("channel")), "beta")
        self.assertEqual(item.find(q("criticalUpdate")).text, None)

    def test_pub_date_only_when_given(self):
        self.run_cli(add_args(self.feed, 7, "1.0.0", extra=["--pub-date", "Sun, 08 Nov 2026 10:00:00 +0000"]))
        self.assertEqual(self.items()[0].findtext("pubDate"), "Sun, 08 Nov 2026 10:00:00 +0000")

    def test_pub_date_now_writes_the_current_utc_time_in_the_exact_format(self):
        with mock.patch.object(appcast, "now_utc", return_value=FIXED_NOW):
            code, _, err = self.run_cli(add_args(self.feed, 7, "1.0.0", extra=["--pub-date", "now"]))
        self.assertEqual(code, 0, err)
        self.assertEqual(self.items()[0].findtext("pubDate"), FIXED_NOW_TEXT)

    def test_pub_date_now_with_the_real_clock(self):
        before = datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0)
        self.assertEqual(self.run_cli(add_args(self.feed, 7, "1.0.0", extra=["--pub-date", "now"]))[0], 0)
        after = datetime.datetime.now(datetime.timezone.utc)
        text = self.items()[0].findtext("pubDate")
        self.assertRegex(text, EXACT_DATE)
        self.assertTrue(before <= email.utils.parsedate_to_datetime(text) <= after, (before, text, after))

    def test_pub_date_now_is_converted_to_utc(self):
        berlin = datetime.timezone(datetime.timedelta(hours=1))
        local = datetime.datetime(2026, 11, 8, 23, 30, 0, tzinfo=berlin)
        with mock.patch.object(appcast, "now_utc", return_value=local):
            self.run_cli(add_args(self.feed, 7, "1.0.0", extra=["--pub-date", "now"]))
        self.assertEqual(self.items()[0].findtext("pubDate"), "Sun, 08 Nov 2026 22:30:00 +0000")

    def test_pub_date_now_has_english_names_in_any_locale(self):
        old = locale.setlocale(locale.LC_TIME)
        for name in ("de_DE.UTF-8", "fr_FR.UTF-8", "ja_JP.UTF-8", "de_DE", "fr_FR"):
            try:
                locale.setlocale(locale.LC_TIME, name)
                break
            except locale.Error:
                continue
        else:
            self.skipTest("no non-English locale installed")
        try:
            self.assertNotEqual(datetime.date(2026, 11, 8).strftime("%a"), "Sun")
            with mock.patch.object(appcast, "now_utc", return_value=FIXED_NOW):
                self.run_cli(add_args(self.feed, 7, "1.0.0", extra=["--pub-date", "now"]))
        finally:
            locale.setlocale(locale.LC_TIME, old)
        self.assertEqual(self.items()[0].findtext("pubDate"), FIXED_NOW_TEXT)

    def test_pub_date_names_are_a_fixed_english_table(self):
        utc = datetime.timezone.utc
        months = [appcast.format_pub_date(datetime.datetime(2026, m, 1, 0, 0, 0, tzinfo=utc)).split(" ")[2]
                  for m in range(1, 13)]
        self.assertEqual(months, ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"])
        days = [appcast.format_pub_date(datetime.datetime(2026, 11, d, 0, 0, 0, tzinfo=utc)).split(",")[0]
                for d in range(2, 9)]
        self.assertEqual(days, ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"])

    def test_an_explicit_pub_date_in_the_exact_format_is_kept(self):
        for text in ("Sun, 08 Nov 2026 10:00:00 +0000", "Thu, 01 Jan 2026 00:00:00 +0000",
                     "Wed, 31 Dec 2025 23:59:59 +0000"):
            with self.subTest(text):
                if os.path.exists(self.feed):
                    os.remove(self.feed)
                self.assertEqual(self.run_cli(add_args(self.feed, 7, "1.0.0", extra=["--pub-date", text]))[0], 0)
                self.assertEqual(self.items()[0].findtext("pubDate"), text)

    def test_any_other_pub_date_form_is_refused_and_writes_nothing(self):
        for label, text in NOT_EXACT_DATES.items():
            with self.subTest(label):
                if os.path.exists(self.feed):
                    os.remove(self.feed)
                code, _, err = self.run_cli(add_args(self.feed, 7, "1.0.0", extra=["--pub-date", text]))
                self.assertEqual(code, 1, err)
                self.assertIn("pubDate", err)
                self.assertFalse(os.path.exists(self.feed))

    def test_no_pub_date_flag_still_leaves_the_element_out(self):
        self.run_cli(add_args(self.feed, 7, "1.0.0"))
        self.assertIsNone(self.items()[0].find("pubDate"))

    def test_rollout_zero_omits_element(self):
        self.run_cli(add_args(self.feed, 7, "1.0.0", extra=["--rollout", "0"]))
        self.assertIsNone(self.items()[0].find(q("phasedRolloutInterval")))

    def test_sorted_newest_first(self):
        for v, s in ((10, "1.0.0"), (30, "1.2.0"), (20, "1.1.0")):
            self.run_cli(add_args(self.feed, v, s))
        self.assertEqual(self.versions(), ["30", "20", "10"])

    def test_numeric_not_lexical_sort(self):
        for v, s in ((9, "1.0.0"), (10, "1.0.1")):
            self.run_cli(add_args(self.feed, v, s))
        self.assertEqual(self.versions(), ["10", "9"])

    def test_duplicate_version_refused_and_feed_unchanged(self):
        self.run_cli(add_args(self.feed, 7, "1.0.0"))
        before = self.read()
        code, _, err = self.run_cli(add_args(self.feed, 7, "1.0.1"))
        self.assertEqual(code, 1)
        self.assertIn("7", err)
        self.assertEqual(self.read(), before)

    def test_rejects_bad_inputs_and_writes_nothing(self):
        cases = {
            "http url": ["--url", "http://example.com/a.dmg"],
            "ftp url": ["--url", "ftp://example.com/a.dmg"],
            "notes http": ["--notes-url", "http://example.com/n"],
            "zero length": ["--length", "0"],
            "text length": ["--length", "big"],
            "bad signature": ["--ed-signature", "not base64!"],
            "short signature": ["--ed-signature", base64.b64encode(b"abc").decode()],
            "bad short": ["--short", "1.0"],
            "bad min system": ["--min-system", "thirteen"],
            "bad channel": ["--channel", "nightly"],
            "negative rollout": ["--rollout", "-5"],
            "bad pub date": ["--pub-date", "yesterday"],
            "zero version": ["--version", "0"],
            "text version": ["--version", "1.2"],
        }
        for name, override in cases.items():
            with self.subTest(name):
                args = add_args(self.feed, 7, "1.0.0")
                code, _, err = self.run_cli(args + override)
                self.assertEqual(code, 1, err)
                self.assertTrue(err.strip(), name)
                self.assertFalse(os.path.exists(self.feed), name)

    def test_unreadable_feed_is_never_overwritten(self):
        with open(self.feed, "w", encoding="utf-8") as f:
            f.write("this is not xml")
        code, _, _ = self.run_cli(add_args(self.feed, 7, "1.0.0"))
        self.assertEqual(code, 1)
        self.assertEqual(self.read(), "this is not xml")

    def test_deterministic_output(self):
        self.build_sample()
        first = self.read()
        os.remove(self.feed)
        self.run_cli(add_args(self.feed, 16, "1.3.0-beta.2", SIG_A, "beta",
                              ["--rollout", "3600", "--critical", "--pub-date", "Sun, 08 Nov 2026 10:00:00 +0000"]))
        self.run_cli(add_args(self.feed, 14, "1.2.1", SIG_C))
        self.run_cli(add_args(self.feed, 15, "1.3.0-beta.1", SIG_B, "beta"))
        self.run_cli(add_args(self.feed, 12, "1.2.0"))
        self.assertEqual(self.read(), first)

    def test_no_timestamp_without_pub_date(self):
        self.run_cli(add_args(self.feed, 7, "1.0.0"))
        self.assertNotIn("pubDate", self.read())
        self.assertNotIn("lastBuildDate", self.read())

    def test_golden_after_adds(self):
        self.build_sample()
        self.assertEqual(self.read(), self.golden("appcast-beta.xml"))

    def test_existing_items_survive_an_add(self):
        self.build_sample()
        before = {i.findtext(q("version")): canon(i) for i in self.items()}
        self.run_cli(add_args(self.feed, 17, "1.3.0-beta.3", SIG_B, "beta"))
        after = {i.findtext(q("version")): canon(i) for i in self.items()}
        for v, xml in before.items():
            self.assertEqual(after[v], xml)


class TestPromote(CliCase):
    def test_removes_channel_keeps_everything_else(self):
        self.build_sample()
        item_before = [i for i in self.items() if i.findtext(q("version")) == "16"][0]
        enc_before = canon(item_before.find("enclosure"))
        code, _, _ = self.run_cli(["promote", "--feed", self.feed, "--version", "16"])
        self.assertEqual(code, 0)
        item = [i for i in self.items() if i.findtext(q("version")) == "16"][0]
        self.assertIsNone(item.find(q("channel")))
        self.assertEqual(canon(item.find("enclosure")), enc_before)
        self.assertEqual(item.find("enclosure").get(q("edSignature")), SIG_A)
        self.assertEqual(item.findtext(q("phasedRolloutInterval")), "3600")
        self.assertIsNotNone(item.find(q("criticalUpdate")))
        self.assertEqual(item.findtext(q("shortVersionString")), "1.3.0-beta.2")

    def test_other_items_unchanged_and_order_kept(self):
        self.build_sample()
        before = {i.findtext(q("version")): canon(i) for i in self.items()}
        self.run_cli(["promote", "--feed", self.feed, "--version", "15"])
        self.assertEqual(self.versions(), ["16", "15", "14", "12"])
        for i in self.items():
            v = i.findtext(q("version"))
            if v != "15":
                self.assertEqual(canon(i), before[v])

    def test_not_found(self):
        self.build_sample()
        before = self.read()
        code, _, err = self.run_cli(["promote", "--feed", self.feed, "--version", "99"])
        self.assertEqual(code, 1)
        self.assertIn("99", err)
        self.assertEqual(self.read(), before)

    def test_already_stable(self):
        self.build_sample()
        before = self.read()
        code, _, err = self.run_cli(["promote", "--feed", self.feed, "--version", "14"])
        self.assertEqual(code, 1)
        self.assertIn("stable", err.lower())
        self.assertEqual(self.read(), before)

    def test_missing_feed(self):
        code, _, err = self.run_cli(["promote", "--feed", self.feed, "--version", "1"])
        self.assertEqual(code, 1)
        self.assertTrue(err.strip())
        self.assertFalse(os.path.exists(self.feed))

    def test_promote_twice_fails_second_time(self):
        self.build_sample()
        self.assertEqual(self.run_cli(["promote", "--feed", self.feed, "--version", "15"])[0], 0)
        self.assertEqual(self.run_cli(["promote", "--feed", self.feed, "--version", "15"])[0], 1)

    def test_short_override_renames_display_string_and_title(self):
        self.build_sample()
        code, _, _ = self.run_cli(["promote", "--feed", self.feed, "--version", "15", "--short", "1.3.0"])
        self.assertEqual(code, 0)
        item = [i for i in self.items() if i.findtext(q("version")) == "15"][0]
        self.assertEqual(item.findtext(q("shortVersionString")), "1.3.0")
        self.assertEqual(item.findtext("title"), "Notchling 1.3.0")

    def test_short_override_must_be_stable_form(self):
        self.build_sample()
        before = self.read()
        code, _, _ = self.run_cli(["promote", "--feed", self.feed, "--version", "15", "--short", "1.3.0-beta.9"])
        self.assertEqual(code, 1)
        self.assertEqual(self.read(), before)

    def test_golden_after_promote(self):
        self.build_sample()
        self.run_cli(["promote", "--feed", self.feed, "--version", "15"])
        self.assertEqual(self.read(), self.golden("appcast-promoted.xml"))

    def test_verify_passes_after_promote(self):
        self.build_sample()
        self.run_cli(["promote", "--feed", self.feed, "--version", "16"])
        self.assertEqual(self.run_cli(["verify", "--feed", self.feed])[0], 0)

    NOTES = "https://github.com/dtthaison/notchling/releases/tag/v1.3.0"

    def item(self, version):
        return [i for i in self.items() if i.findtext(q("version")) == str(version)][0]

    def promote(self, version, *extra):
        return self.run_cli(["promote", "--feed", self.feed, "--version", str(version), *extra])

    def test_notes_url_replaces_the_release_notes_link(self):
        self.build_sample()
        code, _, err = self.promote(15, "--notes-url", self.NOTES)
        self.assertEqual(code, 0, err)
        self.assertEqual(self.item(15).findtext(q("releaseNotesLink")), self.NOTES)
        self.assertIsNone(self.item(15).find(q("channel")))
        self.assertEqual(self.item(16).findtext(q("releaseNotesLink")),
                         "https://github.com/dtthaison/notchling/releases/tag/v1.3.0-beta.2")

    def test_notes_url_must_be_https(self):
        self.build_sample()
        before = self.read()
        for bad in ("http://github.com/n", "ftp://x/y", "https://", "https://a b", ""):
            with self.subTest(bad):
                code, _, err = self.promote(15, "--notes-url", bad)
                self.assertEqual(code, 1)
                self.assertIn("releaseNotesLink", err)
                self.assertEqual(self.read(), before)

    def test_pub_date_now_replaces_an_existing_pub_date(self):
        self.build_sample()
        self.assertEqual(self.item(16).findtext("pubDate"), "Sun, 08 Nov 2026 10:00:00 +0000")
        with mock.patch.object(appcast, "now_utc", return_value=FIXED_NOW):
            code, _, err = self.promote(16, "--pub-date", "now")
        self.assertEqual(code, 0, err)
        self.assertEqual(self.item(16).findtext("pubDate"), FIXED_NOW_TEXT)
        self.assertEqual(len(self.item(16).findall("pubDate")), 1)

    def test_pub_date_is_added_after_the_title_when_the_item_has_none(self):
        self.build_sample()
        self.assertIsNone(self.item(15).find("pubDate"))
        self.assertEqual(self.promote(15, "--pub-date", "Sun, 08 Nov 2026 11:00:00 +0000")[0], 0)
        tags = [c.tag for c in self.item(15)]
        self.assertEqual(tags[:3], ["title", "pubDate", q("version")])
        self.assertEqual(self.item(15).findtext("pubDate"), "Sun, 08 Nov 2026 11:00:00 +0000")

    def test_pub_date_other_forms_are_refused_and_the_feed_is_untouched(self):
        self.build_sample()
        before = self.read()
        for label, text in NOT_EXACT_DATES.items():
            with self.subTest(label):
                code, _, err = self.promote(15, "--pub-date", text)
                self.assertEqual(code, 1, err)
                self.assertIn("pubDate", err)
                self.assertEqual(self.read(), before)

    def test_rollout_replaces_the_interval(self):
        self.build_sample()
        self.assertEqual(self.promote(16, "--rollout", "7200")[0], 0)
        self.assertEqual(self.item(16).findtext(q("phasedRolloutInterval")), "7200")
        self.assertEqual(len(self.item(16).findall(q("phasedRolloutInterval"))), 1)

    def test_rollout_zero_removes_the_interval(self):
        self.build_sample()
        self.assertEqual(self.promote(15, "--rollout", "0")[0], 0)
        self.assertIsNone(self.item(15).find(q("phasedRolloutInterval")))
        self.assertIsNone(self.item(15).find(q("channel")))

    def test_rollout_is_added_in_place_when_the_item_has_none(self):
        self.assertEqual(self.run_cli(add_args(self.feed, 15, "1.3.0-beta.1", SIG_B, "beta",
                                               ["--rollout", "0", "--critical"]))[0], 0)
        self.assertIsNone(self.item(15).find(q("phasedRolloutInterval")))
        self.assertEqual(self.promote(15, "--rollout", "3600")[0], 0)
        tags = [c.tag for c in self.item(15)]
        self.assertEqual(tags.index(q("phasedRolloutInterval")), tags.index(q("minimumSystemVersion")) + 1)
        self.assertEqual(tags.index(q("criticalUpdate")), tags.index(q("phasedRolloutInterval")) + 1)
        self.assertEqual(self.run_cli(["verify", "--feed", self.feed])[0], 0)

    def test_bad_rollout_is_refused_and_the_feed_is_untouched(self):
        self.build_sample()
        before = self.read()
        for bad in ("-5", "abc", "007", "1.5", "", "1e3"):
            with self.subTest(bad):
                code, _, err = self.promote(15, "--rollout", bad)
                self.assertEqual(code, 1)
                self.assertIn("phasedRolloutInterval", err)
                self.assertEqual(self.read(), before)

    def test_without_the_new_flags_the_other_fields_are_left_alone(self):
        self.build_sample()
        before = self.item(16)
        notes, rollout, date = (before.findtext(q("releaseNotesLink")), before.findtext(q("phasedRolloutInterval")),
                                before.findtext("pubDate"))
        self.promote(16)
        after = self.item(16)
        self.assertEqual((after.findtext(q("releaseNotesLink")), after.findtext(q("phasedRolloutInterval")),
                          after.findtext("pubDate")), (notes, rollout, date))

    BETA_URL = ("https://github.com/dtthaison/notchling/releases/download/v1.3.0-beta.1/"
                "Notchling-1.3.0-beta.1.dmg")

    def test_expectations_that_match_promote(self):
        self.build_sample()
        code, _, err = self.promote(15, "--expect-url", self.BETA_URL, "--expect-short", "1.3.0-beta.1")
        self.assertEqual(code, 0, err)
        self.assertIsNone(self.item(15).find(q("channel")))

    def test_each_expectation_works_alone(self):
        self.build_sample()
        self.assertEqual(self.promote(15, "--expect-short", "1.3.0-beta.1")[0], 0)
        self.assertEqual(self.promote(16, "--expect-url", self.BETA_URL.replace("beta.1", "beta.2"))[0], 0)

    def test_a_different_enclosure_url_is_refused_and_the_feed_is_untouched(self):
        self.build_sample()
        before = self.read()
        code, _, err = self.promote(15, "--expect-url", self.BETA_URL.replace("Notchling-1.3.0", "Other-1.3.0"),
                                    "--expect-short", "1.3.0-beta.1", "--short", "1.3.0", "--rollout", "0",
                                    "--notes-url", self.NOTES, "--pub-date", "now")
        self.assertEqual(code, 1)
        self.assertIn(self.BETA_URL, err)
        self.assertIn("Other-1.3.0", err)
        self.assertEqual(self.read(), before)

    def test_a_different_short_version_is_refused_and_the_feed_is_untouched(self):
        self.build_sample()
        before = self.read()
        code, _, err = self.promote(15, "--expect-url", self.BETA_URL, "--expect-short", "1.3.0-beta.2")
        self.assertEqual(code, 1)
        self.assertIn("1.3.0-beta.1", err)
        self.assertIn("1.3.0-beta.2", err)
        self.assertEqual(self.read(), before)

    def test_the_version_number_pointing_at_another_build_is_refused(self):
        self.build_sample()
        before = self.read()
        code, _, _ = self.promote(16, "--expect-url", self.BETA_URL, "--expect-short", "1.3.0-beta.1")
        self.assertEqual(code, 1)
        self.assertEqual(self.read(), before)

    def test_short_is_compared_before_the_short_override(self):
        self.build_sample()
        self.assertEqual(self.promote(15, "--expect-short", "1.3.0-beta.1", "--short", "1.3.0")[0], 0)
        self.assertEqual(self.item(15).findtext(q("shortVersionString")), "1.3.0")

    def test_expecting_the_stable_short_of_a_beta_item_is_refused(self):
        self.build_sample()
        before = self.read()
        self.assertEqual(self.promote(15, "--expect-short", "1.3.0")[0], 1)
        self.assertEqual(self.read(), before)

    def test_all_flags_together_golden(self):
        self.build_sample()
        with mock.patch.object(appcast, "now_utc", return_value=FIXED_NOW):
            code, _, err = self.promote(15, "--short", "1.3.0", "--notes-url", self.NOTES, "--pub-date", "now",
                                        "--rollout", "86400", "--expect-url", self.BETA_URL,
                                        "--expect-short", "1.3.0-beta.1")
        self.assertEqual(code, 0, err)
        self.assertEqual(self.read(), self.golden("appcast-promoted-release.xml"))
        self.assertEqual(self.run_cli(["verify", "--feed", self.feed])[0], 0)

    def test_a_promoted_item_matches_one_added_as_stable_with_the_same_fields(self):
        self.run_cli(add_args(self.feed, 15, "1.3.0-beta.1", SIG_B, "beta", ["--rollout", "0"]))
        self.promote(15, "--short", "1.3.0", "--notes-url", self.NOTES, "--pub-date", "Sun, 08 Nov 2026 11:00:00 +0000",
                     "--rollout", "86400")
        promoted = self.read()
        os.remove(self.feed)
        self.run_cli(add_args(self.feed, 15, "1.3.0", SIG_B, None,
                              ["--url", self.BETA_URL, "--notes-url", self.NOTES,
                               "--pub-date", "Sun, 08 Nov 2026 11:00:00 +0000"]))
        self.assertEqual(promoted, self.read())


class TestVerify(CliCase):
    def write(self, text):
        with open(self.feed, "w", encoding="utf-8") as f:
            f.write(text)

    def sample_text(self):
        self.build_sample()
        return self.read()

    def test_passes_on_generated_feed(self):
        self.build_sample()
        code, out, err = self.run_cli(["verify", "--feed", self.feed])
        self.assertEqual((code, err), (0, ""))
        self.assertIn("4", out)

    def test_passes_on_empty_channel(self):
        self.write(self.golden("appcast-empty.xml"))
        self.assertEqual(self.run_cli(["verify", "--feed", self.feed])[0], 0)

    def test_missing_file(self):
        code, _, err = self.run_cli(["verify", "--feed", self.feed])
        self.assertEqual(code, 1)
        self.assertTrue(err.strip())

    def test_not_xml(self):
        self.write("nope")
        code, _, err = self.run_cli(["verify", "--feed", self.feed])
        self.assertEqual(code, 1)
        self.assertIn("xml", err.lower())

    def test_wrong_root(self):
        self.write('<?xml version="1.0"?><feed/>')
        self.assertEqual(self.run_cli(["verify", "--feed", self.feed])[0], 1)

    def test_missing_sparkle_namespace_declaration(self):
        self.write('<?xml version="1.0"?><rss version="2.0"><channel><title>x</title><link>https://x</link></channel></rss>')
        text = self.sample_text().replace("xmlns:sparkle=", "xmlns:other=")
        self.write(text)
        code, _, err = self.run_cli(["verify", "--feed", self.feed])
        self.assertEqual(code, 1)
        self.assertTrue(err.strip())

    def mutate(self, old, new, count=1):
        text = self.sample_text()
        self.assertIn(old, text)
        self.write(text.replace(old, new, count))
        return self.run_cli(["verify", "--feed", self.feed])

    def test_http_enclosure_url(self):
        code, _, err = self.mutate('url="https://github.com', 'url="http://github.com')
        self.assertEqual(code, 1)
        self.assertIn("https", err)

    def test_http_notes_link(self):
        code, _, err = self.mutate(">https://github.com/dtthaison/notchling/releases/tag", ">http://github.com/dtthaison/notchling/releases/tag")
        self.assertEqual(code, 1)
        self.assertIn("https", err)

    def test_missing_signature(self):
        code, _, err = self.mutate(' sparkle:edSignature="%s"' % SIG_B, "")
        self.assertEqual(code, 1)
        self.assertIn("edSignature", err)

    def test_bad_signature_length(self):
        code, _, err = self.mutate(SIG_B, base64.b64encode(b"short").decode())
        self.assertEqual(code, 1)
        self.assertIn("edSignature", err)

    def test_zero_length(self):
        code, _, err = self.mutate('length="1234567"', 'length="0"')
        self.assertEqual(code, 1)
        self.assertIn("length", err)

    def test_wrong_enclosure_type(self):
        code, _, err = self.mutate('type="application/octet-stream"', 'type="text/plain"')
        self.assertEqual(code, 1)
        self.assertIn("type", err)

    def test_missing_version(self):
        text = self.sample_text()
        self.write(text.replace("<sparkle:version>12</sparkle:version>", ""))
        code, _, err = self.run_cli(["verify", "--feed", self.feed])
        self.assertEqual(code, 1)
        self.assertIn("version", err)

    def test_missing_min_system(self):
        text = self.sample_text()
        self.write(text.replace("<sparkle:minimumSystemVersion>13.0</sparkle:minimumSystemVersion>", "", 1))
        code, _, err = self.run_cli(["verify", "--feed", self.feed])
        self.assertEqual(code, 1)
        self.assertIn("minimumSystemVersion", err)

    def test_unknown_channel_value(self):
        code, _, err = self.mutate("<sparkle:channel>beta</sparkle:channel>", "<sparkle:channel>nightly</sparkle:channel>")
        self.assertEqual(code, 1)
        self.assertIn("channel", err)

    def test_non_integer_version(self):
        code, _, err = self.mutate("<sparkle:version>12</sparkle:version>", "<sparkle:version>1.2</sparkle:version>")
        self.assertEqual(code, 1)
        self.assertIn("version", err)

    def test_duplicate_versions(self):
        code, _, err = self.mutate("<sparkle:version>12</sparkle:version>", "<sparkle:version>14</sparkle:version>")
        self.assertEqual(code, 1)
        self.assertIn("duplicate", err.lower())

    def test_unsorted(self):
        text = self.sample_text()
        text = text.replace("<sparkle:version>16</sparkle:version>", "<sparkle:version>X</sparkle:version>")
        text = text.replace("<sparkle:version>12</sparkle:version>", "<sparkle:version>16</sparkle:version>")
        text = text.replace("<sparkle:version>X</sparkle:version>", "<sparkle:version>12</sparkle:version>")
        self.write(text)
        code, _, err = self.run_cli(["verify", "--feed", self.feed])
        self.assertEqual(code, 1)
        self.assertIn("sorted", err.lower())

    def test_reports_every_problem(self):
        text = self.sample_text()
        text = text.replace('length="1234567"', 'length="0"')
        text = text.replace('url="https://', 'url="http://')
        self.write(text)
        code, _, err = self.run_cli(["verify", "--feed", self.feed])
        self.assertEqual(code, 1)
        self.assertGreater(len(err.strip().splitlines()), 4)

    def test_missing_channel_title(self):
        code, _, err = self.mutate("<title>Notchling</title>", "")
        self.assertEqual(code, 1)
        self.assertIn("title", err)

    def test_empty_phased_interval(self):
        code, _, err = self.mutate("<sparkle:phasedRolloutInterval>3600</sparkle:phasedRolloutInterval>",
                                   "<sparkle:phasedRolloutInterval>soon</sparkle:phasedRolloutInterval>")
        self.assertEqual(code, 1)
        self.assertIn("phasedRolloutInterval", err)

    def test_bad_pub_date(self):
        code, _, err = self.mutate("<pubDate>Sun, 08 Nov 2026 10:00:00 +0000</pubDate>", "<pubDate>yesterday</pubDate>")
        self.assertEqual(code, 1)
        self.assertIn("pubDate", err)

    def test_other_rfc_822_pub_dates_still_pass(self):
        code, _, err = self.mutate("<pubDate>Sun, 08 Nov 2026 10:00:00 +0000</pubDate>",
                                   "<pubDate>Sun, 08 Nov 2026 10:00:00 GMT</pubDate>")
        self.assertEqual((code, err), (0, ""))


class TestCli(CliCase):
    def test_no_command(self):
        with self.assertRaises(SystemExit) as cm:
            with contextlib.redirect_stderr(io.StringIO()):
                appcast.main([])
        self.assertEqual(cm.exception.code, 2)

    def test_atomic_write_leaves_no_temp_files(self):
        self.run_cli(add_args(self.feed, 7, "1.0.0"))
        self.assertEqual(os.listdir(self.tmp.name), ["appcast.xml"])

    def test_file_ends_with_newline_and_has_declaration(self):
        self.run_cli(add_args(self.feed, 7, "1.0.0"))
        text = self.read()
        self.assertTrue(text.startswith('<?xml version="1.0" encoding="utf-8"?>\n'))
        self.assertTrue(text.endswith("</rss>\n"))


if __name__ == "__main__":
    unittest.main()
