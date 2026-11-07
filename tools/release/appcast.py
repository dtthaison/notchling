#!/usr/bin/env python3
"""Build and check Notchling's Sparkle 2 appcast (the update feed). Python 3.9+, standard library only.

    appcast.py add     --feed F --version N --short X.Y.Z[-beta.N] --url https://... --length BYTES
                       --ed-signature B64 --min-system 13.0 --notes-url https://...
                       [--channel beta] [--rollout 86400] [--critical] [--pub-date now|DATE]
    appcast.py promote --feed F --version N [--short X.Y.Z] [--notes-url https://...]
                       [--pub-date now|DATE] [--rollout SECONDS] [--expect-url URL --expect-short X.Y.Z-beta.N]
    appcast.py verify  --feed F

Exit codes: 0 ok, 1 refused or invalid (reasons on stderr), 2 usage error (argparse). A refused command
never touches the feed: writes go to a temp file that replaces the feed in one step, and a feed that
can't be parsed is never overwritten.

Ed25519 signing is NOT done here. Python's standard library has no ed25519, so the signature is made by
Sparkle's `sign_update` in the sign job and passed in as `--ed-signature`. This tool only checks that
it is base64 of 64 bytes (the size of an Ed25519 signature), which catches a misplaced argument.

Sparkle 2.6 layout, which is what the output follows. Children of <item>: title, pubDate (only when
given; Sparkle does not need it), sparkle:version (CFBundleVersion, an integer here), sparkle:shortVersionString,
sparkle:channel (absent = Stable, "beta" = Beta), sparkle:minimumSystemVersion, sparkle:phasedRolloutInterval
(seconds), sparkle:criticalUpdate (empty element), sparkle:releaseNotesLink, enclosure. Attributes of
<enclosure>: url, length, type, sparkle:edSignature. Items are sorted newest first by sparkle:version,
the number Sparkle compares. Output is deterministic: fixed child and attribute order, two-space
indent, no timestamps.

Promotion keeps the enclosure and signature byte for byte (the same dmg goes from Beta to Stable), so
the channel element is removed and nothing else changes unless asked. The dmg's own
CFBundleShortVersionString stays the Beta string, as the bytes are not rebuilt; `promote --short` changes
the feed's display string and title only, and verify does not tie the string to the channel. A Stable
release also gets its own notes link (`--notes-url`), a fresh pubDate (`--pub-date`) and its own rollout
(`--rollout`, 0 removes the element); the elements are added where `add` writes them when the item has none.
`--expect-url` and `--expect-short` name the Beta the caller means to promote: unless the item's enclosure
url and its current (not yet renamed) shortVersionString are exactly those, nothing is written.

pubDate is written and accepted in one form only, `Sun, 08 Nov 2026 10:00:00 +0000` (UTC, English day and
month names whatever the locale, the weekday matching the date); `--pub-date now` is the current UTC time.
`verify` still accepts any RFC 822 date, since Sparkle reads them all.

The feed is our own file, so it is parsed with xml.etree (which resolves no external entities).
"""

import argparse
import base64
import binascii
import datetime
import email.utils
import os
import re
import sys
import tempfile
import xml.etree.ElementTree as ET

SPARKLE_NS = "http://www.andymatuschak.org/xml-namespaces/sparkle"
ET.register_namespace("sparkle", SPARKLE_NS)

FEED_TITLE = "Notchling"
FEED_LINK = "https://github.com/dtthaison/notchling"
FEED_DESCRIPTION = "Notchling updates"
ENCLOSURE_TYPE = "application/octet-stream"
CHANNELS = ("beta",)
DEFAULT_ROLLOUT = 86400

SHORT_RE = re.compile(r"^\d+\.\d+\.\d+(-beta\.\d+)?$")
STABLE_RE = re.compile(r"^\d+\.\d+\.\d+$")
MIN_SYSTEM_RE = re.compile(r"^\d+\.\d+(\.\d+)?$")
INT_RE = re.compile(r"^[1-9]\d*$")


class AppcastError(Exception):
    pass


def q(name):
    return "{%s}%s" % (SPARKLE_NS, name)


# --- checks shared by add and verify -------------------------------------------------------------


def https_problem(label, value):
    if not value or re.search(r"\s", value) or not value.startswith("https://") or len(value) <= len("https://"):
        return "%s must be an https URL without spaces, got %r" % (label, value)
    return None


def signature_problem(value):
    if not value:
        return "sparkle:edSignature is missing"
    try:
        raw = base64.b64decode(value, validate=True)
    except (binascii.Error, ValueError):
        return "sparkle:edSignature is not base64"
    if len(raw) != 64:
        return "sparkle:edSignature must decode to 64 bytes (Ed25519), got %d" % len(raw)
    return None


def positive_int_problem(label, value):
    if value is None or not INT_RE.match(value.strip()):
        return "%s must be a positive integer, got %r" % (label, value)
    return None


def pub_date_problem(value):
    try:
        email.utils.parsedate_to_datetime(value)
    except (TypeError, ValueError, IndexError):
        return "pubDate must be an RFC 822 date, got %r" % value
    return None


# Fixed tables, not strftime: %a and %b follow the locale of whoever runs this, and a feed with "So, 08 Nov"
# is not RFC 822.
DAY_NAMES = ("Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun")
MONTH_NAMES = ("Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec")
PUB_DATE_RE = re.compile(r"(%s), ([0-9]{2}) (%s) ([0-9]{4}) ([0-9]{2}):([0-9]{2}):([0-9]{2}) \+0000"
                         % ("|".join(DAY_NAMES), "|".join(MONTH_NAMES)))


def now_utc():
    return datetime.datetime.now(datetime.timezone.utc)


def format_pub_date(moment):
    """`Sun, 08 Nov 2026 10:00:00 +0000`: UTC, English names, whatever the locale."""
    moment = moment.astimezone(datetime.timezone.utc)
    return "%s, %02d %s %04d %02d:%02d:%02d +0000" % (
        DAY_NAMES[moment.weekday()], moment.day, MONTH_NAMES[moment.month - 1], moment.year,
        moment.hour, moment.minute, moment.second)


def exact_pub_date_problem(value):
    """None when `value` is exactly what format_pub_date writes for a real moment, else the reason."""
    m = PUB_DATE_RE.fullmatch(value)
    if m:
        try:
            moment = datetime.datetime(int(m.group(4)), MONTH_NAMES.index(m.group(3)) + 1, int(m.group(2)),
                                       int(m.group(5)), int(m.group(6)), int(m.group(7)),
                                       tzinfo=datetime.timezone.utc)
        except ValueError:
            m = None
        else:
            m = m if format_pub_date(moment) == value else None  # the weekday must be that day's
    if not m:
        return "pubDate must look like 'Sun, 08 Nov 2026 10:00:00 +0000' (UTC, a real date) or be 'now', got %r" % value
    return None


def resolve_pub_date(value):
    """(text to write or None, problem or None): `now` is the current time, anything else the exact form."""
    if value is None:
        return None, None
    if value == "now":
        return format_pub_date(now_utc()), None
    problem = exact_pub_date_problem(value)
    return (None, problem) if problem else (value, None)


def check_fields(version, short, url, length, signature, min_system, notes_url, channel, rollout, pub_date):
    """Reasons an item's values are unusable; the same rules `add` applies and `verify` re-checks."""
    problems = []
    for problem in (
        positive_int_problem("sparkle:version", version),
        https_problem("enclosure url", url),
        positive_int_problem("enclosure length", length),
        signature_problem(signature),
        https_problem("sparkle:releaseNotesLink", notes_url),
    ):
        if problem:
            problems.append(problem)
    if short is None or not SHORT_RE.match(short):
        problems.append("sparkle:shortVersionString must look like X.Y.Z or X.Y.Z-beta.N, got %r" % short)
    if min_system is None or not MIN_SYSTEM_RE.match(min_system):
        problems.append("sparkle:minimumSystemVersion must look like 13.0, got %r" % min_system)
    if channel is not None and channel not in CHANNELS:
        problems.append("sparkle:channel must be one of %s, got %r" % (", ".join(CHANNELS), channel))
    if rollout is not None and not re.match(r"^[1-9]\d*$", rollout):
        problems.append("sparkle:phasedRolloutInterval must be a positive integer of seconds, got %r" % rollout)
    if pub_date is not None:
        problem = pub_date_problem(pub_date)
        if problem:
            problems.append(problem)
    return problems


# --- reading and writing -------------------------------------------------------------------------


def new_feed():
    rss = ET.Element("rss", {"version": "2.0"})
    channel = ET.SubElement(rss, "channel")
    ET.SubElement(channel, "title").text = FEED_TITLE
    ET.SubElement(channel, "link").text = FEED_LINK
    ET.SubElement(channel, "description").text = FEED_DESCRIPTION
    return rss


def load(path):
    try:
        return ET.parse(path).getroot()
    except FileNotFoundError:
        raise AppcastError("%s does not exist" % path)
    except (ET.ParseError, OSError) as e:
        raise AppcastError("%s is not valid XML: %s" % (path, e))


def channel_of(root):
    channel = root.find("channel") if root.tag == "rss" else None
    if channel is None:
        raise AppcastError("the feed is not an <rss><channel> document")
    return channel


def item_version(item):
    """The integer sparkle:version of an item, or None when absent or not an integer."""
    text = item.findtext(q("version"))
    return int(text) if text is not None and INT_RE.match(text.strip()) else None


def serialize(root):
    ET.indent(root, space="  ")
    return '<?xml version="1.0" encoding="utf-8"?>\n' + ET.tostring(root, encoding="unicode") + "\n"


def write_atomic(path, text):
    directory = os.path.dirname(os.path.abspath(path))
    fd, tmp = tempfile.mkstemp(prefix=".appcast-", suffix=".tmp", dir=directory)
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as f:
            f.write(text)
        os.replace(tmp, path)
    except BaseException:
        if os.path.exists(tmp):
            os.unlink(tmp)
        raise


def sort_items(channel):
    """Newest first by sparkle:version, other channel children (title, link, ...) stay in front."""
    items = channel.findall("item")
    others = [c for c in list(channel) if c.tag != "item"]
    items.sort(key=lambda i: item_version(i) or 0, reverse=True)
    for child in list(channel):
        channel.remove(child)
    for child in others + items:
        channel.append(child)


# --- commands ------------------------------------------------------------------------------------


def make_title(short):
    return "%s %s" % (FEED_TITLE, short)


# Children of <item> in the order `add` writes them; promote puts anything it has to add in the same place.
ITEM_ORDER = ("title", "pubDate", q("version"), q("shortVersionString"), q("channel"), q("minimumSystemVersion"),
              q("phasedRolloutInterval"), q("criticalUpdate"), q("releaseNotesLink"), "enclosure")


def set_child(item, tag, text):
    """Set the text of an item's child, creating it in ITEM_ORDER's place when the item has none."""
    child = item.find(tag)
    if child is None:
        rank = ITEM_ORDER.index(tag)
        position = len(item)
        for index, other in enumerate(item):
            if other.tag in ITEM_ORDER and ITEM_ORDER.index(other.tag) > rank:
                position = index
                break
        child = ET.Element(tag)
        item.insert(position, child)
    child.text = text


def rollout_problem(value):
    if not re.fullmatch(r"[1-9][0-9]*", value):
        return "sparkle:phasedRolloutInterval must be a positive integer of seconds, got %r" % value
    return None


def cmd_add(a):
    pub_date, date_problem = resolve_pub_date(a.pub_date)
    # check_fields gets no pubDate: add holds it to the exact form, which verify does not ask of other feeds.
    problems = check_fields(a.version, a.short, a.url, a.length, a.ed_signature, a.min_system, a.notes_url,
                            a.channel, None if a.rollout == "0" else a.rollout, None)
    if date_problem:
        problems.append(date_problem)
    if problems:
        raise AppcastError("\n".join(problems))

    root = load(a.feed) if os.path.exists(a.feed) else new_feed()
    channel = channel_of(root)
    version = int(a.version)
    if any(item_version(i) == version for i in channel.findall("item")):
        raise AppcastError("version %d is already in the feed" % version)

    item = ET.Element("item")
    ET.SubElement(item, "title").text = make_title(a.short)
    if pub_date:
        ET.SubElement(item, "pubDate").text = pub_date
    ET.SubElement(item, q("version")).text = str(version)
    ET.SubElement(item, q("shortVersionString")).text = a.short
    if a.channel:
        ET.SubElement(item, q("channel")).text = a.channel
    ET.SubElement(item, q("minimumSystemVersion")).text = a.min_system
    if a.rollout != "0":
        ET.SubElement(item, q("phasedRolloutInterval")).text = str(int(a.rollout))
    if a.critical:
        ET.SubElement(item, q("criticalUpdate"))
    ET.SubElement(item, q("releaseNotesLink")).text = a.notes_url
    enclosure = ET.SubElement(item, "enclosure")
    enclosure.set("url", a.url)
    enclosure.set("length", str(int(a.length)))
    enclosure.set("type", ENCLOSURE_TYPE)
    enclosure.set(q("edSignature"), a.ed_signature)
    channel.append(item)

    sort_items(channel)
    write_atomic(a.feed, serialize(root))
    print("added version %d (%s) to %s" % (version, a.short, a.feed))


def cmd_promote(a):
    problem = positive_int_problem("--version", a.version)
    if problem:
        raise AppcastError(problem)
    problems = []
    if a.short is not None and not STABLE_RE.match(a.short):
        problems.append("--short must look like X.Y.Z (a Stable version), got %r" % a.short)
    if a.notes_url is not None:
        problem = https_problem("sparkle:releaseNotesLink", a.notes_url)
        if problem:
            problems.append(problem)
    if a.rollout is not None and a.rollout != "0":
        problem = rollout_problem(a.rollout)
        if problem:
            problems.append(problem)
    pub_date, date_problem = resolve_pub_date(a.pub_date)
    if date_problem:
        problems.append(date_problem)
    if problems:
        raise AppcastError("\n".join(problems))

    root = load(a.feed)
    channel = channel_of(root)
    version = int(a.version)
    matches = [i for i in channel.findall("item") if item_version(i) == version]
    if not matches:
        raise AppcastError("version %d is not in the feed" % version)
    item = matches[0]
    marker = item.find(q("channel"))
    if marker is None:
        raise AppcastError("version %d is already stable" % version)

    # Everything is checked before the first change; the feed is only written at the end.
    enclosure = item.find("enclosure")
    actual_url = enclosure.get("url") if enclosure is not None else None
    old_short = item.findtext(q("shortVersionString"))
    if a.expect_url is not None and actual_url != a.expect_url:
        problems.append("version %d has enclosure url %r, expected %r" % (version, actual_url, a.expect_url))
    if a.expect_short is not None and old_short != a.expect_short:
        problems.append("version %d has shortVersionString %r, expected %r" % (version, old_short, a.expect_short))
    if a.short is not None and item.find(q("shortVersionString")) is None:
        problems.append("version %d has no sparkle:shortVersionString to rename" % version)
    if problems:
        raise AppcastError("not promoting: " + "\n".join(problems))

    item.remove(marker)
    if a.short is not None:
        item.find(q("shortVersionString")).text = a.short
        title = item.find("title")
        if title is not None and title.text == make_title(old_short):
            title.text = make_title(a.short)
    if a.notes_url is not None:
        set_child(item, q("releaseNotesLink"), a.notes_url)
    if pub_date is not None:
        set_child(item, "pubDate", pub_date)
    if a.rollout == "0":
        for old in item.findall(q("phasedRolloutInterval")):
            item.remove(old)
    elif a.rollout is not None:
        set_child(item, q("phasedRolloutInterval"), str(int(a.rollout)))
    write_atomic(a.feed, serialize(root))
    print("promoted version %d to stable in %s" % (version, a.feed))


def verify_feed(root):
    """Every reason the feed is not a good Sparkle 2 appcast; an empty list means it is."""
    problems = []
    if root.tag != "rss":
        return ["the root element must be <rss>, got <%s>" % root.tag]
    if root.get("version") != "2.0":
        problems.append('<rss> must have version="2.0"')
    channels = root.findall("channel")
    if len(channels) != 1:
        return problems + ["the feed must have exactly one <channel>, found %d" % len(channels)]
    channel = channels[0]
    for name in ("title", "link"):
        if not (channel.findtext(name) or "").strip():
            problems.append("<channel> has no <%s>" % name)
    link = channel.findtext("link")
    if link and https_problem("<channel> link", link):
        problems.append(https_problem("<channel> link", link))

    seen = {}
    order = []
    for index, item in enumerate(channel.findall("item"), 1):
        where = "item %d" % index
        version_text = item.findtext(q("version"))
        version = item_version(item)
        if version is not None:
            where = "item %d (version %d)" % (index, version)
            if version in seen:
                problems.append("%s: duplicate version, also item %d" % (where, seen[version]))
            else:
                seen[version] = index
            order.append(version)
        short = item.findtext(q("shortVersionString"))
        enclosure = item.find("enclosure")
        if enclosure is None:
            problems.append("%s: no <enclosure>" % where)
            fields = {}
        else:
            fields = enclosure.attrib
            if enclosure.get("type") != ENCLOSURE_TYPE:
                problems.append("%s: enclosure type must be %s, got %r" % (where, ENCLOSURE_TYPE, enclosure.get("type")))
        channel_text = item.findtext(q("channel"))
        rollout = item.findtext(q("phasedRolloutInterval"))
        pub_date = item.findtext("pubDate")
        notes = item.findtext(q("releaseNotesLink"))
        checks = check_fields(
            version_text if version_text is not None else None, short,
            fields.get("url"), fields.get("length"), fields.get(q("edSignature")),
            item.findtext(q("minimumSystemVersion")), notes,
            channel_text, rollout, pub_date)
        if enclosure is None:
            # the enclosure fields are all missing; one reason is enough
            checks = [c for c in checks if not c.startswith(("enclosure", "sparkle:edSignature"))]
        for c in checks:
            problems.append("%s: %s" % (where, c))
        if item.find(q("channel")) is not None and channel_text is None:
            problems.append("%s: sparkle:channel is empty" % where)
        if version_text is None:
            problems.append("%s: sparkle:version is missing" % where)
    if order != sorted(order, reverse=True):
        problems.append("items are not sorted newest first by sparkle:version: %s" % order)
    return problems


def cmd_verify(a):
    root = load(a.feed)
    problems = verify_feed(root)
    if problems:
        raise AppcastError("\n".join(problems))
    count = len(channel_of(root).findall("item"))
    print("%s ok: %d item%s" % (a.feed, count, "" if count == 1 else "s"))


# --- command line --------------------------------------------------------------------------------


def parser():
    p = argparse.ArgumentParser(prog="appcast.py", description="Build and check Notchling's Sparkle appcast.")
    sub = p.add_subparsers(dest="command", required=True)

    add = sub.add_parser("add", help="add an item (creates the feed if missing)")
    add.add_argument("--feed", required=True)
    add.add_argument("--version", required=True, help="CFBundleVersion, a positive integer")
    add.add_argument("--short", required=True, help="X.Y.Z or X.Y.Z-beta.N")
    add.add_argument("--url", required=True, help="https URL of the dmg")
    add.add_argument("--length", required=True, help="dmg size in bytes")
    add.add_argument("--ed-signature", required=True, help="base64 Ed25519 signature from sign_update")
    add.add_argument("--min-system", required=True, help="LSMinimumSystemVersion, e.g. 13.0")
    add.add_argument("--notes-url", required=True, help="https URL of the release notes")
    add.add_argument("--channel", default=None, help="beta; leave out for Stable")
    add.add_argument("--rollout", default=str(DEFAULT_ROLLOUT), help="phased rollout seconds; 0 leaves it out")
    add.add_argument("--critical", action="store_true", help="mark as a critical update")
    add.add_argument("--pub-date", default=None,
                     help="'now' (current UTC time) or 'Sun, 08 Nov 2026 10:00:00 +0000' exactly; left out unless given")
    add.set_defaults(run=cmd_add)

    promote = sub.add_parser("promote", help="move a Beta item to Stable (same enclosure and signature)")
    promote.add_argument("--feed", required=True)
    promote.add_argument("--version", required=True)
    promote.add_argument("--short", default=None, help="display version for Stable, X.Y.Z")
    promote.add_argument("--notes-url", default=None, help="https URL of the Stable release notes; replaces the Beta's")
    promote.add_argument("--pub-date", default=None,
                         help="'now' or 'Sun, 08 Nov 2026 10:00:00 +0000' exactly; replaces the Beta's pubDate")
    promote.add_argument("--rollout", default=None,
                         help="phased rollout seconds, set or replaced; 0 removes the element")
    promote.add_argument("--expect-url", default=None,
                         help="refuse unless the item's enclosure url is exactly this")
    promote.add_argument("--expect-short", default=None,
                         help="refuse unless the item's current shortVersionString is exactly this (X.Y.Z-beta.N)")
    promote.set_defaults(run=cmd_promote)

    verify = sub.add_parser("verify", help="check structure, fields, https URLs and order")
    verify.add_argument("--feed", required=True)
    verify.set_defaults(run=cmd_verify)
    return p


def main(argv=None):
    args = parser().parse_args(argv)
    try:
        args.run(args)
    except AppcastError as e:
        print(str(e), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
