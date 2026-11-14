from __future__ import annotations

import json
import shutil
import tempfile
from pathlib import Path
from typing import Dict, Optional

FIXTURES = Path(__file__).resolve().parent / "fixtures"
TREE = FIXTURES / "tree"
GOLDEN = Path(__file__).resolve().parent / "golden"


class Workdir:
    def __init__(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.path = Path(self._tmp.name)
        self.locales = self.path / "locales"

    def cleanup(self) -> None:
        self._tmp.cleanup()

    def copy_tree(self) -> Path:
        shutil.copytree(TREE, self.locales)
        return self.locales

    def write(self, rel: str, text: str) -> Path:
        target = self.locales / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding="utf-8")
        return target

    def config(self, source: str = "en", ship=("en",), draft=(), outputs=("rust", "arb")) -> None:
        self.write(
            "languages.json",
            json.dumps(
                {"source": source, "ship": list(ship), "draft": list(draft), "outputs": list(outputs)}
            ),
        )


def small_tree(files: Dict[str, str], ship=("en",), draft=(), outputs=("rust", "arb"), work: Optional[Workdir] = None) -> Workdir:
    work = work or Workdir()
    work.config(ship=ship, draft=draft, outputs=outputs)
    for rel, text in files.items():
        work.write(rel, text)
    return work
