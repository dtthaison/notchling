"""Tests for tools/release/appcast.py: no network, no signing, only temp files."""

import base64
import contextlib
import io
import os
import tempfile
import unittest
import xml.etree.ElementTree as ET

import appcast

SPARKLE = "http://www.andymatuschak.org/xml-namespaces/sparkle"
GOLDEN = os.path.join(os.path.dirname(__file__), "golden")

# A real Ed25519 signature is 64 bytes. These are fixed made-up ones, never keys.
SIG_A = base64.b64encode(bytes(range(64))).decode()
SIG_B = base64.b64encode(bytes(range(64, 128))).decode()
SIG_C = base64.b64encode(bytes(range(128, 192))).decode()


def q(name):
    return "{%s}%s" % (SPARKLE, name)


def canon(element):
    """Serialized form without the tail, which depends on where the element sits."""
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
        """Stable 12 and 14, Beta 15 and 16; the second Beta has every optional field."""
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
        # In Sparkle 2.6 the version strings, channel, minimum system, rollout, critical flag and notes
        # link are children of item; only url, length, type and edSignature are enclosure attributes.
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
                # later flags win in argparse, so appending overrides
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
        # Rebuilding from scratch in a different order gives the same bytes.
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
        # ElementTree cannot tell a missing declaration from none needed, so check an item uses it.
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
        # Swap two versions' numbers so the order on disk is wrong but each stays unique.
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
