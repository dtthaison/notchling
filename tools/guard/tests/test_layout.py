from __future__ import annotations

import json
import re
import subprocess
import sys
import unittest
from pathlib import Path
from typing import Dict, List, Tuple

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools" / "guard"))

from guard.zones import Zones, load  # noqa: E402

ZONES_BEFORE = Path(__file__).resolve().parent / "fixtures" / "zones-before-phase4.json"

SWIFT_MOVED = ["Sources", "Tests", "Demo", "Icon", "Pet", "Info.plist", "release.sh", "build.sh"]
PHONE_OLD, PHONE_NEW = "mobile", "apps/flutter"
SWIFT_NEW = "legacy/macos"

HISTORY = (
    "docs/superpowers/", "docs/plans/", "docs/decisions/", "CHANGELOG.md",
    "tools/guard/tests/fixtures/notes-before-phase3.txt",
    "tools/guard/tests/fixtures/zones-before-phase4.json",
    "tools/guard/tests/test_layout.py",
)
OWN_FOLDERS = (SWIFT_NEW + "/", PHONE_NEW + "/")

OLD_SWIFT = re.compile(r"(?<![\w/])(?:Sources/(?:Notchling|ConsoleCore|hostd|AGENTS\.md)|Tests/(?:NotchlingTests|ConsoleCoreTests))")
OLD_PHONE = re.compile(r"(?:^|[^/\w])mobile/")


def tracked() -> List[str]:
    out = subprocess.run(["git", "-C", str(ROOT), "ls-files", "-z"], check=True, capture_output=True).stdout
    return [p for p in out.decode("utf-8").split("\0") if p]


def text_of(path: str):
    try:
        data = (ROOT / path).read_bytes()
    except OSError:
        return None
    if b"\0" in data:
        return None
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError:
        return None


def new_path(old: str) -> str:
    for name in SWIFT_MOVED:
        if old == name or old.startswith(name + "/"):
            return SWIFT_NEW + "/" + old
    if old.startswith(PHONE_OLD + "/"):
        return PHONE_NEW + old[len(PHONE_OLD):]
    return old


def survivors(before: Dict[str, List[bool]], have) -> List[Tuple[str, str, List[bool]]]:
    return [(old, new_path(old), flags) for old, flags in before.items() if new_path(old) in have]


def zone_diffs(before: Dict[str, List[bool]], zones: Zones, have) -> List[str]:
    diffs = []
    for old, new, (in_zone, new_ok) in survivors(before, have):
        now = (zones.in_zone(new), zones.new_file_ok(new))
        if now != (in_zone, new_ok):
            diffs.append(f"{old} -> {new}: was {(in_zone, new_ok)}, now {now}")
    return diffs


def zone_counts(before: Dict[str, List[bool]], zones: Zones, have) -> Tuple[Tuple[int, int], Tuple[int, int]]:
    alive = survivors(before, have)
    return (
        (sum(zones.in_zone(new) for _, new, _ in alive), sum(flags[0] for _, _, flags in alive)),
        (sum(zones.new_file_ok(new) for _, new, _ in alive), sum(flags[1] for _, _, flags in alive)),
    )


class LayoutTests(unittest.TestCase):
    def setUp(self):
        self.files = tracked()

    def test_old_paths_are_gone(self):
        gone = [p for p in self.files if p != "build.sh" and any(p == n or p.startswith(n + "/") for n in SWIFT_MOVED)]
        gone += [p for p in self.files if p.startswith(PHONE_OLD + "/")]
        self.assertEqual(gone, [])

    def test_new_paths_exist(self):
        want = [
            "Sources/Notchling", "Sources/ConsoleCore", "Sources/hostd", "Sources/AGENTS.md", "Tests/NotchlingTests",
            "Tests/ConsoleCoreTests", "Demo/make-demo.sh", "Icon", "Pet", "Info.plist", "release.sh", "build.sh",
        ]
        have = set(self.files)
        for w in want:
            full = SWIFT_NEW + "/" + w
            self.assertTrue(full in have or any(p.startswith(full + "/") for p in have), full)
        for w in ["pubspec.yaml", "AGENTS.md", "DESIGN.md", "lib", "test", "tool"]:
            full = PHONE_NEW + "/" + w
            self.assertTrue(full in have or any(p.startswith(full + "/") for p in have), full)

    def test_root_keeps_package_and_viz(self):
        for p in ["Package.swift", "build.sh", "viz/viz.js"]:
            self.assertIn(p, self.files)

    def test_root_build_sh_is_a_wrapper(self):
        text = (ROOT / "build.sh").read_text()
        code = [ln for ln in text.splitlines() if ln.strip() and not ln.lstrip().startswith("#")]
        self.assertLessEqual(len(code), 4, code)
        self.assertTrue(any("exec" in ln and "legacy/macos/build.sh" in ln and '"$@"' in ln for ln in code), code)

    def test_package_swift_points_into_legacy(self):
        text = (ROOT / "Package.swift").read_text()
        paths = re.findall(r'path: "([^"]+)"', text)
        self.assertTrue(paths)
        for p in paths:
            self.assertTrue(p.startswith(SWIFT_NEW + "/"), p)
            self.assertTrue((ROOT / p).is_dir(), p)

    def test_no_old_path_is_named_outside_the_moved_folders(self):
        hits: List[Tuple[str, int, str]] = []
        for path in self.files:
            if path.startswith(HISTORY) or path.startswith(OWN_FOLDERS):
                continue
            text = text_of(path)
            if text is None:
                continue
            for n, line in enumerate(text.splitlines(), 1):
                if OLD_SWIFT.search(line) or OLD_PHONE.search(line):
                    hits.append((path, n, line.strip()[:100]))
        self.assertEqual(hits, [], "\n".join(f"{p}:{n}: {s}" for p, n, s in hits))

    def test_moved_folders_do_not_name_the_other_side_by_its_old_path(self):
        hits = []
        for path in self.files:
            if not path.startswith(OWN_FOLDERS) or path.startswith(HISTORY):
                continue
            text = text_of(path)
            if text is None:
                continue
            for n, line in enumerate(text.splitlines(), 1):
                if OLD_PHONE.search(line):
                    hits.append(f"{path}:{n}: {line.strip()[:100]}")
        self.assertEqual(hits, [], "\n".join(hits))

    def test_zone_covers_the_same_files_up_to_the_move(self):
        before: Dict[str, List[bool]] = json.loads(ZONES_BEFORE.read_text())["files"]
        zones = load(ROOT / "tools" / "guard" / "zones.json")
        have = set(self.files)
        self.assertGreater(len(before), 700)
        self.assertGreater(len(survivors(before, have)), len(before) // 2)
        diffs = zone_diffs(before, zones, have)
        self.assertEqual(diffs, [], "\n".join(diffs[:20]))

    def test_zone_count_is_unchanged(self):
        before = json.loads(ZONES_BEFORE.read_text())["files"]
        zones = load(ROOT / "tools" / "guard" / "zones.json")
        (now_zone, was_zone), (now_new, was_new) = zone_counts(before, zones, set(self.files))
        self.assertEqual(now_zone, was_zone)
        self.assertEqual(now_new, was_new)


class ZoneCompareTests(unittest.TestCase):

    ZONES = Zones(("docs/**", "!docs/rules.md", "legacy/macos/Sources/Notchling/Chat/Views/**"), ("legacy/macos/Tests/**",))
    BEFORE = {
        "docs/a.md": [True, False],
        "docs/b.md": [True, False],
        "Sources/Notchling/Chat/Views/V.swift": [True, False],
        "Tests/NotchlingTests/T.swift": [False, True],
    }
    ALL = {"docs/a.md", "docs/b.md", "legacy/macos/Sources/Notchling/Chat/Views/V.swift", "legacy/macos/Tests/NotchlingTests/T.swift"}

    def test_nothing_deleted_nothing_flipped(self):
        self.assertEqual(zone_diffs(self.BEFORE, self.ZONES, self.ALL), [])
        self.assertEqual(zone_counts(self.BEFORE, self.ZONES, self.ALL), ((3, 3), (1, 1)))

    def test_a_deleted_file_is_not_a_failure(self):
        have = self.ALL - {"docs/a.md", "legacy/macos/Tests/NotchlingTests/T.swift"}
        self.assertEqual(zone_diffs(self.BEFORE, self.ZONES, have), [])
        self.assertEqual(zone_counts(self.BEFORE, self.ZONES, have), ((2, 2), (0, 0)))

    def test_a_file_whose_zone_flag_flipped_still_fails(self):
        before = dict(self.BEFORE, **{"docs/rules.md": [True, False]})
        have = self.ALL | {"docs/rules.md"}
        diffs = zone_diffs(before, self.ZONES, have)
        self.assertEqual(len(diffs), 1, diffs)
        self.assertIn("docs/rules.md", diffs[0])
        self.assertEqual(zone_counts(before, self.ZONES, have), ((3, 4), (1, 1)))

    def test_a_new_file_flag_that_flipped_still_fails(self):
        before = dict(self.BEFORE, **{"Tests/NotchlingTests/T.swift": [False, False]})
        diffs = zone_diffs(before, self.ZONES, self.ALL)
        self.assertEqual(len(diffs), 1, diffs)
        self.assertEqual(zone_counts(before, self.ZONES, self.ALL)[1], (1, 0))

    def test_a_flipped_file_is_still_caught_when_another_one_is_deleted(self):
        before = dict(self.BEFORE, **{"docs/rules.md": [True, False]})
        have = (self.ALL | {"docs/rules.md"}) - {"docs/b.md"}
        self.assertEqual(len(zone_diffs(before, self.ZONES, have)), 1)
        self.assertEqual(zone_counts(before, self.ZONES, have)[0], (2, 3))


if __name__ == "__main__":
    unittest.main()
