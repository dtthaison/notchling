import hashlib
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", ".."))

import gen_sound

PINNED = "de54d628f26dae7cbc9f35a6a26f3ceb05fec2144a9e11f1adbf5fc3e5d812f6"


class GenSoundTest(unittest.TestCase):
    def test_same_bytes_twice(self):
        self.assertEqual(gen_sound.render(), gen_sound.render())

    def test_pinned_hash(self):
        self.assertEqual(hashlib.sha256(gen_sound.render()).hexdigest(), PINNED)

    def test_is_a_short_aiff(self):
        data = gen_sound.render()
        self.assertEqual(data[:4], b"FORM")
        self.assertEqual(data[8:12], b"AIFF")
        frames = len(gen_sound.samples())
        self.assertTrue(0.3 < frames / gen_sound.RATE < 0.5)

    def test_writes_the_path_it_is_given(self):
        with tempfile.TemporaryDirectory() as folder:
            path = os.path.join(folder, "stopped.aiff")
            self.assertEqual(gen_sound.main(["gen_sound.py", path]), 0)
            with open(path, "rb") as handle:
                self.assertEqual(handle.read(), gen_sound.render())


if __name__ == "__main__":
    unittest.main()
