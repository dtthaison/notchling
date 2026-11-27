from __future__ import annotations

from typing import Optional, Tuple

SLASH = {".swift", ".rs", ".dart", ".js", ".ts", ".kt", ".java", ".c", ".h", ".m"}
HASH = {".sh", ".py", ".rb", ".yml", ".yaml", ".toml"}


def _ext(path: str) -> str:
    dot = path.rfind(".")
    return path[dot:].lower() if dot > path.rfind("/") else ""


def _rust_char_literal(line: str, i: int) -> bool:
    if line[i + 1:i + 2] == "\\":
        return True
    return line[i + 2:i + 3] == "'"


def strip_comments(path: str, line: str) -> str:
    return strip_code(path, line, keep_strings=True)


class _Scan:
    def __init__(self, ext: str, line: str, keep_strings: bool) -> None:
        self.line, self.n, self.keep = line, len(line), keep_strings
        self.out = list(line)
        self.swift, self.rust, self.dart = ext == ".swift", ext == ".rs", ext == ".dart"
        self.slash, self.hash = ext in SLASH, ext in HASH
        self.quotes = "\"" if self.swift or self.rust else "\"'"

    def _blank(self, a: int, b: int) -> None:
        for k in range(a, min(b, self.n)):
            self.out[k] = " "

    def _blank_string(self, a: int, b: int) -> None:
        if not self.keep:
            self._blank(a, b)

    def code(self, i: int, closer: str = "") -> Tuple[int, bool]:
        line, n = self.line, self.n
        opener = {")": "(", "}": "{"}.get(closer, "")
        depth = 0
        while i < n:
            c = line[i]
            if self.slash and line.startswith("//", i):
                self._blank(i, n)
                return n, False
            if self.slash and line.startswith("/*", i):
                end = line.find("*/", i + 2)
                stop = n if end < 0 else end + 2
                self._blank(i, stop)
                i = stop
                continue
            if self.hash and c == "#":
                self._blank(i, n)
                return n, False
            after = self._skip_string(i)
            if after >= 0:
                i = after
                continue
            if closer:
                if c == opener:
                    depth += 1
                elif c == closer:
                    if depth == 0:
                        return i + 1, True
                    depth -= 1
            i += 1
        return n, False

    def _skip_string(self, i: int) -> int:
        line, c = self.line, self.line[i]
        if self.swift:
            if c == "`":
                end = line.find("`", i + 1)
                return end + 1 if end >= 0 else -1
            if c == "#":
                h = 0
                while line.startswith("#", i + h):
                    h += 1
                if line.startswith("/", i + h):
                    end = line.find("/" + "#" * h, i + h + 1)
                    self._blank_string(i + h + 1, self.n if end < 0 else end)
                    return self.n if end < 0 else end + 1 + h
                if not line.startswith("\"", i + h):
                    return -1
                if line.startswith("\"\"\"", i + h):
                    return i + h + 3
                return self._string(i + h + 1, "\"", h)
        raw = self._raw_open(i) if c in "rb" else None
        if raw:
            quote_at, hashes = raw
            if not self.rust and line.startswith(line[quote_at] * 3, quote_at):
                return quote_at + 3
            return self._string(quote_at + 1, line[quote_at], hashes, raw=True)
        if c in self.quotes:
            if not self.rust and line.startswith(c * 3, i):
                return i + 3
            return self._string(i + 1, c, 0)
        if self.rust and c == "'" and _rust_char_literal(line, i):
            return self._string(i + 1, c, 0)
        return -1

    def _raw_open(self, i: int) -> Optional[Tuple[int, int]]:
        line = self.line
        if not (self.rust or self.dart) or (i > 0 and (line[i - 1].isalnum() or line[i - 1] == "_")):
            return None
        j = i + 2 if self.rust and line.startswith("br", i) else i + 1 if line[i] == "r" else -1
        if j < 0:
            return None
        if self.dart:
            return (j, 0) if line[j:j + 1] in ("'", "\"") else None
        h = 0
        while line.startswith("#", j + h):
            h += 1
        return (j + h, h) if line.startswith("\"", j + h) else None

    def _string(self, i: int, quote: str, hashes: int, raw: bool = False) -> int:
        line, n = self.line, self.n
        end, esc = quote + "#" * hashes, "\\" + "#" * hashes
        start = j = i
        while j < n:
            if raw:
                if line.startswith(end, j):
                    self._blank_string(start, j)
                    return j + len(end)
                j += 1
                continue
            if line.startswith(esc, j):
                k = j + len(esc)
                if self.swift and line.startswith("(", k):
                    j = self._interpolation(start, k + 1, ")")
                    start = j
                else:
                    j = k + 1
                continue
            if self.dart and line.startswith("${", j):
                j = self._interpolation(start, j + 2, "}")
                start = j
                continue
            if line.startswith(end, j):
                self._blank_string(start, j)
                return j + len(end)
            j += 1
        self._blank_string(start, n)
        return n

    def _interpolation(self, start: int, code_from: int, closer: str) -> int:
        self._blank_string(start, code_from)
        after, found = self.code(code_from, closer)
        if found:
            self._blank_string(after - 1, after)
        return after


def strip_code(path: str, line: str, keep_strings: bool = False) -> str:
    ext = _ext(path)
    if ext not in SLASH and ext not in HASH:
        return line
    scan = _Scan(ext, line, keep_strings)
    try:
        scan.code(0)
    except RecursionError:
        return line
    return "".join(scan.out)
