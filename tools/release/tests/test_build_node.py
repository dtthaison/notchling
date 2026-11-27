import os
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SCRIPT = ROOT / "tools" / "build-node.sh"
BASH = "/bin/bash"
LINUX = ("x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl")
MAC = "x86_64-apple-darwin"


class BuildNodeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="nl-build-node-"))
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.home = self.tmp / "home"
        self.home.mkdir()
        self.stubs = self.tmp / "stubs"
        self.stubs.mkdir()
        self.empty = self.tmp / "empty"
        self.empty.mkdir()

    def stub(self, *names):
        for name in names:
            path = self.stubs / name
            path.write_text("#!/bin/sh\nexit 97\n")
            path.chmod(0o755)

    def run_script(self, *args, path, **env):
        full = {"HOME": str(self.home), "PATH": path, **env}
        return subprocess.run(
            [BASH, str(SCRIPT), *args], env=full, capture_output=True, text=True, timeout=60, cwd=self.tmp
        )

    def dry(self, *args, **env):
        self.stub("zig", "cargo-zigbuild")
        result = self.run_script("--dry-run", *args, path=f"{self.stubs}:/usr/bin:/bin", **env)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout.splitlines()

    def test_dry_run_lists_both_linux_targets(self):
        lines = self.dry()
        for target in LINUX:
            self.assertTrue(any(l.startswith(f"build {target}: cargo zigbuild --release --locked "
                                             f"-p notchling-notchlingd --bin notchling --target {target}")
                                for l in lines), lines)
        self.assertIn("copy x86_64-unknown-linux-musl to notchling-linux-x86_64", lines)
        self.assertIn("copy aarch64-unknown-linux-musl to notchling-linux-aarch64", lines)
        self.assertFalse(any(MAC in l for l in lines), lines)
        build = [l for l in lines if l.startswith("NOTCHLING_BUILD=")]
        self.assertEqual(len(build), 1)
        self.assertRegex(build[0], r"^NOTCHLING_BUILD=[0-9a-f]{7,40}(-dirty)?$")

    def test_dry_run_adds_mac_intel_with_flag(self):
        lines = self.dry("--mac-intel")
        self.assertTrue(any(l.startswith(f"build {MAC}: ") and f"--target {MAC}" in l for l in lines), lines)
        self.assertIn(f"copy {MAC} to notchling-macos-x86_64", lines)

    def test_missing_zig_says_how_to_install(self):
        result = self.run_script("--dry-run", path=str(self.empty))
        self.assertEqual(result.returncode, 1)
        self.assertIn("zig", result.stderr)
        self.assertIn("cargo-zigbuild", result.stderr)
        self.assertIn("brew install zig", result.stderr)
        self.assertIn("cargo install --locked cargo-zigbuild", result.stderr)
        self.assertEqual(result.stdout, "")

    def test_missing_cargo_zigbuild_alone_is_named(self):
        self.stub("zig")
        result = self.run_script("--dry-run", path=str(self.stubs))
        self.assertEqual(result.returncode, 1)
        self.assertIn("missing: cargo-zigbuild", result.stderr)

    def test_manifest_written_last(self):
        lines = self.dry("--mac-intel")
        manifest = [i for i, l in enumerate(lines) if l == "write manifest.json"]
        self.assertEqual(manifest, [len(lines) - 1])
        copies = [i for i, l in enumerate(lines) if l.startswith("copy ")]
        builds = [i for i, l in enumerate(lines) if l.startswith("build ")]
        self.assertEqual(len(copies), 3)
        self.assertLess(max(builds), min(copies))
        self.assertLess(max(copies), manifest[0])

    def test_dist_folder_follows_the_data_root(self):
        lines = self.dry(NOTCHLING_DATA_ROOT=str(self.tmp / "root"))
        self.assertIn(f"dist: {self.tmp / 'root'}/notchling/cluster/dist", lines)
        lines = self.dry()
        self.assertIn(f"dist: {self.home}/Library/Application Support/Notchling/cluster/dist", lines)

    def test_dry_run_writes_nothing(self):
        root = self.tmp / "root"
        self.dry("--mac-intel", NOTCHLING_DATA_ROOT=str(root))
        self.assertFalse(root.exists())
        self.assertEqual(list(self.home.iterdir()), [])

    def test_unknown_argument_is_refused(self):
        result = self.run_script("--install", path=str(self.empty))
        self.assertEqual(result.returncode, 2)
        self.assertIn("usage", result.stderr)

    def test_script_is_executable(self):
        self.assertTrue(os.access(SCRIPT, os.X_OK))


if __name__ == "__main__":
    unittest.main()
