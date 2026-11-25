from __future__ import annotations

import os
import unittest

from tests.helpers import GOLDEN


def assert_golden(case: unittest.TestCase, name: str, actual: str) -> None:
    target = GOLDEN / name
    if os.environ.get("NOTCHLING_UPDATE_GOLDEN") == "1":
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(actual, encoding="utf-8")
    case.assertEqual(target.read_text(encoding="utf-8"), actual, f"golden {name} differs; NOTCHLING_UPDATE_GOLDEN=1 rewrites it")
