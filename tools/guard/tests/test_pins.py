import tempfile
import unittest
from pathlib import Path

from guard.pins import check, load_allowlist, peeled_sha, pin

SHA = "0123456789abcdef0123456789abcdef01234567"
ALLOW = {"actions/checkout", "actions/setup-python", "subosito/flutter-action"}


def wf(text):
    d = Path(tempfile.mkdtemp())
    (d / "ci.yml").write_text(text)
    return d


class PinsTests(unittest.TestCase):
    def test_tag_pinned_fails(self):
        out = check(wf("    steps:\n      - uses: actions/checkout@v7\n"), ALLOW)
        self.assertEqual(len(out), 1)
        self.assertTrue(out[0].startswith("GUARD-PINS:"))

    def test_sha_pinned_allowlisted_passes(self):
        self.assertEqual(check(wf(f"      - uses: actions/checkout@{SHA} # v7\n"), ALLOW), [])

    def test_sha_pinned_subpath_passes(self):
        self.assertEqual(check(wf(f"      - uses: actions/setup-python/sub@{SHA}\n"), ALLOW), [])

    def test_unknown_owner_fails(self):
        out = check(wf(f"      - uses: evil/action@{SHA}\n"), ALLOW)
        self.assertEqual(len(out), 1)
        self.assertIn("evil/action", out[0])

    def test_docker_fails(self):
        self.assertEqual(len(check(wf("      - uses: docker://alpine:3.20\n"), ALLOW)), 1)

    def test_local_action_ok(self):
        self.assertEqual(check(wf("      - uses: ./.github/actions/setup\n"), ALLOW), [])

    def test_quoted_uses(self):
        self.assertEqual(len(check(wf("      - uses: 'actions/checkout@v7'\n"), ALLOW)), 1)

    def test_reusable_workflow_job_uses(self):
        self.assertEqual(len(check(wf("  call:\n    uses: evil/flows/.github/workflows/x.yml@main\n"), ALLOW)), 1)

    def test_commented_uses_ignored(self):
        self.assertEqual(check(wf("      # - uses: actions/checkout@v7\n"), ALLOW), [])

    def test_yaml_extension_checked(self):
        d = Path(tempfile.mkdtemp())
        (d / "ci.yaml").write_text("      - uses: actions/checkout@v7\n")
        self.assertEqual(len(check(d, ALLOW)), 1)

    def test_pin_rewrites_with_tag_comment(self):
        d = wf("      - uses: actions/checkout@v7\n      - uses: ./local\n")
        pin(d, lambda repo, tag: SHA)
        self.assertEqual((d / "ci.yml").read_text(),
                         f"      - uses: actions/checkout@{SHA} # v7\n      - uses: ./local\n")
        self.assertEqual(check(d, ALLOW), [])

    def test_peeled_sha_prefers_peeled(self):
        out = f"{'1' * 40}\trefs/tags/v7\n{SHA}\trefs/tags/v7^{{}}\n"
        self.assertEqual(peeled_sha(out), SHA)
        self.assertEqual(peeled_sha(f"{SHA}\trefs/tags/v7\n"), SHA)

    def test_repo_allowlist(self):
        self.assertEqual(load_allowlist(Path(__file__).parents[1] / "actions-allowlist.json"), ALLOW)


if __name__ == "__main__":
    unittest.main()
