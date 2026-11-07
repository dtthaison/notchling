"""Blank out comments and string contents on one source line, so the checks look only at code.

A small scanner, not regexes across quotes: it knows `//` and `/* */` (Swift, Rust, Dart), `#`
(shell, Python), and "..." and '...' strings with backslash escapes. Rust's `'a` lifetimes aren't
strings. The result keeps the line's length, so column positions still line up.
"""

from __future__ import annotations

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


def strip_code(path: str, line: str, keep_strings: bool = False) -> str:
    ext = _ext(path)
    if ext not in SLASH and ext not in HASH:
        return line
    quotes = "\"'" if ext != ".rs" else "\""
    out = list(line)
    i, n = 0, len(line)
    while i < n:
        c = line[i]
        if ext in SLASH and line.startswith("//", i):
            for j in range(i, n):
                out[j] = " "
            break
        if ext in SLASH and line.startswith("/*", i):
            end = line.find("*/", i + 2)
            stop = n if end < 0 else end + 2
            for j in range(i, stop):
                out[j] = " "
            i = stop
            continue
        if ext in HASH and c == "#":
            for j in range(i, n):
                out[j] = " "
            break
        if c in quotes or (ext == ".rs" and c == "'" and _rust_char_literal(line, i)):
            j = i + 1
            while j < n and line[j] != c:
                j += 2 if line[j] == "\\" else 1
            if not keep_strings:
                for k in range(i + 1, min(j, n)):
                    out[k] = " "
            i = j + 1
            continue
        i += 1
    return "".join(out)
