"""Blank out comments and string contents on one source line, so the checks look only at code.

A small scanner, not regexes across quotes. It knows `//` and `/* */` (Swift, Rust, Dart), `#`
(shell, Python), and strings with backslash escapes. The result keeps the line's length, so column
positions still line up. What it takes from each language:

- Swift: only `"` opens a string, with `#"..."#` raw strings, and `\\(...)` inside one is code (the
  scanner reads it as code, counting parentheses). A backtick name is an identifier, not a string;
  `'` is nothing; `#/ ... /#` is a regex literal and may hold quotes (the bare `/ ... /` form is
  off in the build's Swift 5 mode). Dart: `'` and `"`, and `${...}` is code.
- Rust: `"` strings, `r#"..."#` raw strings (also Dart's `r'...'`: no escapes), and `'x'` char
  literals (`'a` is a lifetime).
- Everything else with comments: `'` and `"` strings.
- A triple quote is skipped, not read as a string: the scanner sees one line at a time, and a line
  can close a multiline string and carry code after the delimiter (`\"\"\"; run()`).
- A string that doesn't close on the line is blanked to its end.
"""

from __future__ import annotations

from typing import Optional, Tuple

SLASH = {".swift", ".rs", ".dart", ".js", ".ts", ".kt", ".java", ".c", ".h", ".m"}
HASH = {".sh", ".py", ".rb", ".yml", ".yaml", ".toml"}


def _ext(path: str) -> str:
    dot = path.rfind(".")
    return path[dot:].lower() if dot > path.rfind("/") else ""


def _rust_char_literal(line: str, i: int) -> bool:
    """`'x'` or `'\\n'` is a char literal; `'a` (a lifetime) isn't."""
    if line[i + 1:i + 2] == "\\":
        return True
    return line[i + 2:i + 3] == "'"


def strip_comments(path: str, line: str) -> str:
    """Like `strip_code`, but string contents stay: for patterns that live inside strings
    (`"/usr/bin/security"`, `import 'package:http/...'`)."""
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
        """Scan code from `i`. With a `closer` (`)` or `}`), stop after the one that balances the
        interpolation's own opener. Returns where to go on, and whether the closer was found."""
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
        """If a string, char literal or identifier starts at `i`: where it ends, with its text
        blanked. Otherwise -1."""
        line, c = self.line, self.line[i]
        if self.swift:
            if c == "`":
                end = line.find("`", i + 1)
                return end + 1 if end >= 0 else -1
            if c == "#":
                h = 0  # the number of `#`s before the quote
                while line.startswith("#", i + h):
                    h += 1
                if line.startswith("/", i + h):  # `#/ ... /#`, an extended regex literal: it can hold quotes
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
        """Rust `r"`, `r#"`, `br#"` and Dart `r'`: a raw string has no escapes, so a quote or a
        backslash inside it means nothing. Returns where the opening quote is and how many `#`s
        precede it, or None."""
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
        """`i` is just past the opening quote. Blanks the text, leaves interpolated code, returns
        the index past the closing delimiter (`quote` and `hashes` `#`s)."""
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
        """Blank the string text from `start` up to the interpolation's code (which stays), and the
        closing paren or brace. Returns the index past it."""
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
    except RecursionError:  # strings nested in interpolations nested in strings, a thousand deep
        return line  # nothing blanked: the checks see all of it
    return "".join(scan.out)
