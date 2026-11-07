import unittest

from guard.changes import diff
from tests.helpers import commit, head_of, make_repo, move


class DiffTests(unittest.TestCase):
    def test_added_lines_and_numbers(self):
        repo = make_repo({"a.swift": "one\n"})
        base = head_of(repo)
        head = commit(repo, {"a.swift": "one\ntwo\n"})
        [c] = diff(repo, base, head)
        self.assertEqual((c.path, c.status, c.added), ("a.swift", "M", ((2, "two"),)))

    def test_removed_lines(self):
        repo = make_repo({"a.swift": "one\ntwo\n"})
        base = head_of(repo)
        head = commit(repo, {"a.swift": "one\n"})
        [c] = diff(repo, base, head)
        self.assertEqual((c.added, c.removed), ((), ("two",)))

    def test_new_file(self):
        repo = make_repo({})
        base = head_of(repo)
        head = commit(repo, {"Tests/New.swift": "a\nb\n"})
        [c] = diff(repo, base, head)
        self.assertEqual((c.status, c.added), ("A", ((1, "a"), (2, "b"))))

    def test_rename_keeps_old_path(self):
        repo = make_repo({"Sources/x.swift": "let a = 1\nlet b = 2\nlet c = 3\n"})
        base = head_of(repo)
        head = move(repo, "Sources/x.swift", "Sources/Notchling/Search/x.swift")
        [c] = diff(repo, base, head)
        self.assertEqual((c.path, c.status, c.old_path),
                         ("Sources/Notchling/Search/x.swift", "R", "Sources/x.swift"))

    def test_binary_change_listed(self):
        repo = make_repo({"mobile/test/goldens/a.png": b"\x89PNG\x00\x01"})
        base = head_of(repo)
        head = commit(repo, {"mobile/test/goldens/a.png": b"\x89PNG\x00\x02"})
        [c] = diff(repo, base, head)
        self.assertEqual((c.path, c.binary, c.added), ("mobile/test/goldens/a.png", True, ()))

    def test_deleted_file(self):
        repo = make_repo({"Tests/Old.swift": "x\n"})
        base = head_of(repo)
        head = commit(repo, {"Tests/Old.swift": None})
        [c] = diff(repo, base, head)
        self.assertEqual((c.path, c.status), ("Tests/Old.swift", "D"))

    def test_only_changes_since_merge_base(self):
        repo = make_repo({"a.txt": "a\n"})
        base = head_of(repo)
        head = commit(repo, {"b.txt": "b\n"})
        self.assertEqual([c.path for c in diff(repo, base, head)], ["b.txt"])

    def test_path_with_space(self):
        repo = make_repo({})
        base = head_of(repo)
        head = commit(repo, {"docs/a b.md": "x\n"})
        self.assertEqual([c.path for c in diff(repo, base, head)], ["docs/a b.md"])


if __name__ == "__main__":
    unittest.main()
