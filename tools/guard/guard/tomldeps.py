from __future__ import annotations

from typing import Iterator, List, Optional, Tuple

DEP_KINDS = ("dependencies", "dev-dependencies", "build-dependencies", "dev_dependencies", "build_dependencies")
MACOS_CFG = 'cfg(target_os="macos")'

Segments = List[str]

_ESCAPES = {"b": "\b", "t": "\t", "n": "\n", "f": "\f", "r": "\r", '"': '"', "\\": "\\"}


def _unescape(body: str) -> str:
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
    s = code.strip()
    if not s.startswith("["):
        return None
    opener = 2 if s.startswith("[[") else 1
    for i, c in _outside(s[opener:]):
        if c == "]":
            return _dotted(s[opener:opener + i])
    return None


def _pair(code: str) -> Optional[Tuple[Segments, str]]:
    for i, c in _outside(code):
        if c == "=":
            key = _dotted(code[:i])
            return (key, code[i + 1:].strip()) if key else None
    return None


def _string_value(value: str) -> Optional[str]:
    v = value.strip()
    if not v or v[0] not in "\"'":
        return None
    end = _string_end(v, 0)
    if end != len(v):
        return None
    return _unescape(v[1:-1]) if v[0] == '"' else v[1:-1]


def _inline(value: str) -> Optional[List[Tuple[Segments, str]]]:
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
    entries = _inline(value)
    if entries:
        for key, inner in entries:
            yield from _leaves(path + key, inner)
    else:
        yield path, value


def _dependency_tables(path: Segments) -> Optional[Tuple[int, bool]]:
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
    depth = 0
    for _, c in _outside(_strip_comment(text)):
        if c in "[{":
            depth += 1
        elif c in "]}":
            depth -= 1
    return depth
