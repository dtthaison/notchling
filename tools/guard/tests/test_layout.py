"""Phase 4 moved the Swift app to legacy/macos/ and the phone app to apps/flutter/. These tests pin
the layout: the old paths are gone, the new ones exist, nothing outside the moved folders (and the
history docs) still names an old path, and the auto-merge zone covers the same files as before.
"""

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

from guard.zones import load  # noqa: E402

ZONES_BEFORE = Path(__file__).resolve().parent / "fixtures" / "zones-before-phase4.json"

# What moved into legacy/macos/ (a path prefix, or a whole file at the root), and where the phone went.
SWIFT_MOVED = ["Sources", "Tests", "Demo", "Icon", "Pet", "Info.plist", "release.sh", "build.sh"]
PHONE_OLD, PHONE_NEW = "mobile", "apps/flutter"
SWIFT_NEW = "legacy/macos"

# Text that keeps the old paths on purpose: history, and the fixtures that record the old layout.
HISTORY = (
    "docs/superpowers/", "docs/plans/", "docs/decisions/", "CHANGELOG.md",
    "tools/guard/tests/fixtures/notes-before-phase3.txt",
    "tools/guard/tests/fixtures/zones-before-phase4.json",
    "tools/guard/tests/test_layout.py",
)
# Their own folders read paths relative to themselves ("Sources/Notchling" is right inside legacy/macos/).
OWN_FOLDERS = (SWIFT_NEW + "/", PHONE_NEW + "/")

OLD_SWIFT = re.compile(r"(?<![\w/])(?:Sources/(?:Notchling|ConsoleCore|hostd|AGENTS\.md)|Tests/(?:NotchlingTests|ConsoleCoreTests))")
OLD_PHONE = re.compile(r"(?:^|[^/\w])mobile/")


def tracked() -> List[str]:
    out = subprocess.run(["git", "-C", str(ROOT), "ls-files", "-z"], check=True, capture_output=True).stdout
    return [p for p in out.decode("utf-8").split("\0") if p]


def text_of(path: str):
    """The file's text, or None for a binary one (or one that's gone)."""
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
    """Where a path from before the move lives now."""
    for name in SWIFT_MOVED:
        if old == name or old.startswith(name + "/"):
            return SWIFT_NEW + "/" + old
    if old.startswith(PHONE_OLD + "/"):
        return PHONE_NEW + old[len(PHONE_OLD):]
    return old


class LayoutTests(unittest.TestCase):
    def setUp(self):
        self.files = tracked()

    def test_old_paths_are_gone(self):
        # A new root build.sh exists (the wrapper); every other old path is gone.
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
        """Inside legacy/macos/ the phone is apps/flutter/, never mobile/."""
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
        diffs = []
        for old, (in_zone, new_ok) in before.items():
            new = new_path(old)
            if new not in have:
                if old.startswith("tools/guard/tests/fixtures/") or old == "tools/guard/lint-baseline.json":
                    continue
                diffs.append(f"{old} -> {new}: missing")
                continue
            if (zones.in_zone(new), zones.new_file_ok(new)) != (in_zone, new_ok):
                diffs.append(f"{old} -> {new}: was {(in_zone, new_ok)}, now {(zones.in_zone(new), zones.new_file_ok(new))}")
        self.assertEqual(diffs, [], "\n".join(diffs[:20]))

    def test_zone_count_is_unchanged(self):
        before = json.loads(ZONES_BEFORE.read_text())["files"]
        zones = load(ROOT / "tools" / "guard" / "zones.json")
        moved = [new_path(p) for p in before if new_path(p) in set(self.files)]
        self.assertEqual(sum(zones.in_zone(p) for p in moved), sum(v[0] for v in before.values()))
        self.assertEqual(sum(zones.new_file_ok(p) for p in moved), sum(v[1] for v in before.values()))


if __name__ == "__main__":
    unittest.main()
