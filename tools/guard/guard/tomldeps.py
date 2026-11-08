"""Which lines of a Cargo manifest name a dependency on a crate, outside the tables allowed to.

Python 3.9 has no `tomllib`, so this reads table headers and keys itself, line by line, the way TOML
spells them rather than the way one hand-written manifest happens to:

- keys are dotted (`gpui.workspace = true`, `ui.package = "gpui"`), bare or in `"basic"` or
  `'literal'` quotes (`"gpui" = "1"`, `"\\u0067pui"`), with spaces allowed around the dots;
- a table header is a dotted path too (`[dependencies."gpui"]`, `[ dependencies . gpui ]`), and
  `[dependencies.ui]` followed by `package = "gpui"` is the crate under another name;
- an inline table is read into its entries, however deep (`ui = { version = "1", package = "gpui" }`,
  `dependencies = { gpui = "1" }`), and a value that goes on over several lines (an array, a TOML 1.1
  inline table, a triple-quoted string) is read to its end, so a line inside it is never taken for a key.

A dependency table is `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]` (and the
underscore spellings Cargo still reads), `[workspace.dependencies]`, or one of those three under
`[target.<cfg>]`. It is the macOS one when the cfg is exactly `cfg(target_os = "macos")` (spaces
aside). Only what the rule needs is parsed; a malformed line is skipped, not an error."""

from __future__ import annotations

from typing import Iterator, List, Optional, Tuple

DEP_KINDS = ("dependencies", "dev-dependencies", "build-dependencies", "dev_dependencies", "build_dependencies")
MACOS_CFG = 'cfg(target_os="macos")'

Segments = List[str]

_ESCAPES = {"b": "\b", "t": "\t", "n": "\n", "f": "\f", "r": "\r", '"': '"', "\\": "\\"}


def _unescape(body: str) -> str:
    """A basic string's contents with its escapes resolved (`\\u0067` is a `g`)."""
    out = []
    i = 0
    while i < len(body):
        c = body[i]
        if c != "\\" or i + 1 >= len(body):
            out.append(c)
            i += 1
            continue
        e = body[i + 1]
        if e in _ESCAPES:
            out.append(_ESCAPES[e])
            i += 2
        elif e in "uU":
            width = 4 if e == "u" else 8
            digits = body[i + 2:i + 2 + width]
            try:
                out.append(chr(int(digits, 16)))
            except ValueError:
                out.append(body[i:i + 2 + width])
            i += 2 + width
        else:
            out.append(body[i:i + 2])
            i += 2
    return "".join(out)


def _string_end(text: str, i: int) -> int:
    """The index after the string that opens at `i`, or -1 when it doesn't close in `text`."""
    quote = text[i]
    j = i + 1
    while j < len(text):
        if quote == '"' and text[j] == "\\":
            j += 2
            continue
        if text[j] == quote:
            return j + 1
        j += 1
    return -1


def _outside(text: str) -> Iterator[Tuple[int, str]]:
    """Every (index, character) that is not inside a string, strings skipped whole."""
    i = 0
    while i < len(text):
        c = text[i]
        if c in "\"'":
            end = _string_end(text, i)
            if end < 0:
                return
            i = end
            continue
        yield i, c
        i += 1


def _strip_comment(line: str) -> str:
    for i, c in _outside(line):
        if c == "#":
            return line[:i]
    return line


def _dotted(text: str) -> Optional[Segments]:
    """A dotted key (`a."b.c".'d'`) as its segments; `None` when it isn't one."""
    segments: Segments = []
    i, n = 0, len(text)
    while True:
        while i < n and text[i] in " \t":
            i += 1
        if i >= n:
            return None
        if text[i] in "\"'":
            end = _string_end(text, i)
            if end < 0:
                return None
            body = text[i + 1:end - 1]
            segments.append(_unescape(body) if text[i] == '"' else body)
            i = end
        else:
            j = i
            while j < n and (text[j].isalnum() or text[j] in "-_"):
                j += 1
            if j == i:
                return None
            segments.append(text[i:j])
            i = j
        while i < n and text[i] in " \t":
            i += 1
        if i >= n:
            return segments
        if text[i] != ".":
            return None
        i += 1


def _header(code: str) -> Optional[Segments]:
    """The path of a `[table]` or `[[array of tables]]` line."""
    s = code.strip()
    if not s.startswith("["):
        return None
    opener = 2 if s.startswith("[[") else 1
    for i, c in _outside(s[opener:]):
        if c == "]":
            return _dotted(s[opener:opener + i])
    return None


def _pair(code: str) -> Optional[Tuple[Segments, str]]:
    """A `key = value` line as the key's segments and the value's text."""
    for i, c in _outside(code):
        if c == "=":
            key = _dotted(code[:i])
            return (key, code[i + 1:].strip()) if key else None
    return None


def _string_value(value: str) -> Optional[str]:
    """What a whole-value string says, `None` when the value isn't one string."""
    v = value.strip()
    if not v or v[0] not in "\"'":
        return None
    end = _string_end(v, 0)
    if end != len(v):
        return None
    return _unescape(v[1:-1]) if v[0] == '"' else v[1:-1]


def _inline(value: str) -> Optional[List[Tuple[Segments, str]]]:
    """The entries of an inline table value, `None` when the value isn't one."""
    v = value.strip()
    if not (v.startswith("{") and v.endswith("}")):
        return None
    body = v[1:-1]
    parts: List[str] = []
    depth, start = 0, 0
    for i, c in _outside(body):
        if c in "[{":
            depth += 1
        elif c in "]}":
            depth -= 1
        elif c == "," and depth == 0:
            parts.append(body[start:i])
            start = i + 1
    parts.append(body[start:])
    entries = []
    for part in parts:
        pair = _pair(part)
        if pair:
            entries.append(pair)
    return entries


def _leaves(path: Segments, value: str) -> Iterator[Tuple[Segments, str]]:
    """The value at `path`, or each of its entries' when it is a (non-empty) inline table."""
    entries = _inline(value)
    if entries:
        for key, inner in entries:
            yield from _leaves(path + key, inner)
    else:
        yield path, value


def _dependency_tables(path: Segments) -> Optional[Tuple[int, bool]]:
    """When `path` starts inside a dependency table: how many segments the table is and whether it
    is the macOS target's."""
    for j, segment in enumerate(path):
        if segment not in DEP_KINDS:
            continue
        head = path[:j]
        if head in ([], ["workspace"]):
            return j + 1, False
        if len(head) == 2 and head[0] == "target":
            return j + 1, "".join(head[1].split()) == MACOS_CFG
    return None


def _names(path: Segments, value: Optional[str], crate: str) -> bool:
    """Whether `path` is, in a dependency table that isn't the macOS one, an entry for `crate`: by its
    key (`gpui`, `gpui.version`, a `[dependencies.gpui]` table) or by `package = "gpui"` under any
    name."""
    found = _dependency_tables(path)
    if found is None:
        return False
    size, macos = found
    rest = path[size:]
    if macos or not rest:
        return False
    if rest[0] == crate:
        return True
    return value is not None and rest[1:] == ["package"] and _string_value(value) == crate


def lines_naming_outside_macos(lines: List[str], crate: str) -> List[Tuple[int, str]]:
    """The (1-based line, text) of every line that names `crate` as a dependency outside the macOS
    target tables: a key, a table header, or a `package = "<crate>"` rename."""
    out: List[Tuple[int, str]] = []
    table: Segments = []
    table_flagged = False
    i = 0
    while i < len(lines):
        raw = lines[i]
        line_no = i + 1
        i += 1
        code = _strip_comment(raw)
        header = _header(code)
        if header is not None:
            table = header
            table_flagged = _names(table, None, crate)
            if table_flagged:
                out.append((line_no, raw.strip()))
            continue
        pair = _pair(code)
        if pair is None:
            continue
        key, value = pair
        # A value that goes on over more lines is read to its end.
        for quote in ('"""', "'''"):
            if value.startswith(quote) and value.find(quote, 3) < 0:
                while i < len(lines) and quote not in lines[i]:
                    i += 1
                i += 1
                value = value[:3]
        depth = _depth(value)
        while depth > 0 and i < len(lines):
            more = _strip_comment(lines[i]).strip()
            i += 1
            value = f"{value} {more}"
            depth += _depth(more)
        if table_flagged:
            continue
        if any(_names(table + leaf, text, crate) for leaf, text in _leaves(key, value)):
            out.append((line_no, raw.strip()))
    return out


def _depth(text: str) -> int:
    """How many brackets `text` opens and doesn't close, strings and comments aside."""
    depth = 0
    for _, c in _outside(_strip_comment(text)):
        if c in "[{":
            depth += 1
        elif c in "]}":
            depth -= 1
    return depth
