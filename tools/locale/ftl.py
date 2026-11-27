from __future__ import annotations

import re
import unicodedata
from dataclasses import dataclass, field
from typing import Dict, List, Optional, Tuple, Union

RULE = "GUARD-LOCALE-SUBSET"
CATEGORIES = ("zero", "one", "two", "few", "many")
IDENT = r"[A-Za-z][A-Za-z0-9_-]*"
VAR = r"[a-z][a-z0-9]*(?:_[a-z0-9]+)*"


class FtlError(ValueError):
    def __init__(self, path: str, line: int, message: str, rule: str = RULE):
        super().__init__(message)
        self.rule = rule
        self.path = path
        self.line = line
        self.message = message

    def __str__(self) -> str:
        return f"{self.rule}: {self.path}:{self.line}: {self.message}"


@dataclass(frozen=True)
class Lit:
    text: str


@dataclass(frozen=True)
class Var:
    name: str


@dataclass(frozen=True)
class Num:
    name: str


@dataclass(frozen=True)
class Term:
    name: str


Part = Union[Lit, Var, Num, Term]
Pattern = List[Part]


@dataclass
class Variant:
    category: str
    pattern: Pattern
    default: bool


@dataclass
class Select:
    var: str
    variants: List[Variant]


@dataclass
class Message:
    key: str
    value: Union[Pattern, Select]
    notes: List[str]
    group: Optional[str]
    line: int
    first: int = 0
    last: int = 0


@dataclass
class Resource:
    path: str
    messages: List[Message] = field(default_factory=list)
    terms: Dict[str, str] = field(default_factory=dict)
    term_lines: Dict[str, int] = field(default_factory=dict)
    file_notes: List[str] = field(default_factory=list)


def hidden(ch: str) -> bool:
    if ch == "\n":
        return False
    cat = unicodedata.category(ch)
    if cat in ("Cf", "Cc", "Zl", "Zp"):
        return True
    cp = ord(ch)
    return 0xFE00 <= cp <= 0xFE0F or 0xE0100 <= cp <= 0xE01EF


def parse(text: str, path: str) -> Resource:
    return _Parser(text, path).run()


class _Parser:
    def __init__(self, text: str, path: str):
        self.path = path
        self.text = text
        self.lines = text.split("\n")
        if self.lines and self.lines[-1] == "":
            self.lines.pop()
        self.res = Resource(path)

    def err(self, line: int, message: str) -> FtlError:
        return FtlError(self.path, line, message)

    def prescan(self) -> None:
        if self.text.startswith("﻿"):
            raise self.err(1, "a byte order mark is not allowed")
        for i, line in enumerate(self.lines, 1):
            if "\r" in line:
                raise self.err(i, "CRLF line ends are not allowed; use LF")
            if "\t" in line:
                raise self.err(i, "a tab is not allowed; use spaces")
            for ch in line:
                if hidden(ch):
                    raise self.err(
                        i,
                        f'hidden character U+{ord(ch):04X}; write it as {{"\\u{ord(ch):04x}"}} in a string literal',
                    )
            if line != line.rstrip(" "):
                raise self.err(i, 'a space at the end of a line is not allowed; write a meant one as {" "}')

    def run(self) -> Resource:
        self.prescan()
        notes: List[str] = []
        note_first = 0
        group: Optional[str] = None
        i = 0
        n = len(self.lines)
        while i < n:
            line = self.lines[i]
            no = i + 1
            if line == "":
                notes = []
                i += 1
                continue
            if line.startswith("#"):
                m = re.match(r"^(#{1,3})(?: (.*))?$", line)
                if not m:
                    raise self.err(no, "a comment line is # then a space then the text")
                body = m.group(2) or ""
                level = len(m.group(1))
                if level == 3:
                    self.res.file_notes.append(body)
                    notes = []
                elif level == 2:
                    group = body
                    notes = []
                else:
                    if not notes:
                        note_first = no
                    notes.append(body)
                i += 1
                continue
            if line[0] == " ":
                if re.match(r"^ +\.[A-Za-z]", line):
                    raise self.err(no, "an attribute (.name = ...) is not allowed; use two messages")
                raise self.err(no, "a value must be on one line; a new line is a new message")
            if line.startswith("-"):
                i = self.term(i)
                notes = []
                continue
            m = re.match(rf"^({IDENT}) =(?: (.*))?$", line)
            if not m:
                raise self.err(no, "expected a message (key = text), a term, or a comment")
            key = m.group(1)
            rest = m.group(2)
            first = note_first if notes else no
            if rest is None:
                if i + 1 < n and self.lines[i + 1].startswith(" "):
                    raise self.err(no + 1, "a value must be on one line; a new line is a new message")
                raise self.err(no, "a message needs a value")
            if rest.startswith(" "):
                raise self.err(no, 'a space at the start of a value is not allowed; write a meant one as {" "}')
            sel = re.match(rf"^\{{ \$({VAR}) ->$", rest)
            if sel:
                value, nxt = self.select(i, sel.group(1))
                last = nxt
                i = nxt
            else:
                if rest.startswith("{") and "->" in rest:
                    raise self.err(no, "a select must be the whole value: one select per message")
                value = self.pattern(rest, no, False)
                if i + 1 < n and self.lines[i + 1].startswith(" "):
                    nxt_line = self.lines[i + 1]
                    if re.match(r"^ +\.[A-Za-z]", nxt_line):
                        raise self.err(no + 1, "an attribute (.name = ...) is not allowed; use two messages")
                    raise self.err(no + 1, "a value must be on one line; a new line is a new message")
                last = no
                i += 1
            self.res.messages.append(Message(key, value, list(notes), group, no, first, last))
            notes = []
        return self.res

    def term(self, i: int) -> int:
        no = i + 1
        line = self.lines[i]
        m = re.match(rf"^-({IDENT}) = (\S.*)$", line)
        if not m:
            raise self.err(no, "expected a brand term: -brand-name = Text")
        name = m.group(1)
        if not re.match(r"^brand-[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*$", name):
            raise self.err(no, "only -brand-* terms are allowed")
        pattern = self.pattern(m.group(2), no, False)
        out = ""
        for part in pattern:
            if isinstance(part, Lit):
                out += part.text
            else:
                raise self.err(no, "a brand term is plain text: no variables or other terms")
        if i + 1 < len(self.lines) and self.lines[i + 1].startswith(" "):
            raise self.err(no + 1, "a value must be on one line; a new line is a new message")
        short = name[len("brand-"):]
        self.res.terms[short] = out
        self.res.term_lines[short] = no
        return i + 1

    def select(self, i: int, var: str) -> Tuple[Select, int]:
        header = i + 1
        variants: List[Variant] = []
        seen = set()
        j = i + 1
        n = len(self.lines)
        has_other = False
        while True:
            if j >= n:
                raise self.err(max(j, header), "unclosed select: the closing } is missing")
            line = self.lines[j]
            no = j + 1
            if line == "":
                raise self.err(no - 1, "unclosed select: the closing } is missing")
            if re.match(r"^ *\}$", line):
                break
            if line.startswith("}"):
                raise self.err(no, "one select per message: nothing may follow the closing }")
            if not line.startswith(" "):
                raise self.err(no - 1, "unclosed select: the closing } is missing")
            m = re.match(r"^ +(\*?)\[([^\]]*)\] (.*)$", line)
            if not m:
                if re.match(r"^ +\.", line):
                    raise self.err(no, "an attribute (.name = ...) is not allowed; use two messages")
                raise self.err(no, "expected a variant: [one] text or *[other] text")
            star, cat, body = m.groups()
            if re.match(r"^[0-9]+$", cat):
                raise self.err(
                    no, f"an exact-number variant [{cat}] is not allowed; use a plural category or its own message"
                )
            if cat != "other" and cat not in CATEGORIES:
                raise self.err(
                    no, f"[{cat}] is not a plural category; use zero, one, two, few, many or *[other]"
                )
            if cat == "other" and not star:
                raise self.err(no, "the default variant is written *[other]")
            if star and cat != "other":
                raise self.err(no, "only *[other] is the default variant")
            if cat in seen:
                raise self.err(no, f"variant [{cat}] appears twice")
            seen.add(cat)
            if body.startswith(" "):
                raise self.err(no, 'a space at the start of a value is not allowed; write a meant one as {" "}')
            if body == "":
                raise self.err(no, "a variant needs text")
            pattern = self.pattern(body, no, True)
            variants.append(Variant(cat, pattern, cat == "other"))
            if cat == "other":
                has_other = True
            j += 1
        if not has_other:
            raise self.err(header, "a select needs a default variant, *[other]")
        return Select(var, variants), j + 1

    def pattern(self, text: str, no: int, in_variant: bool) -> Pattern:
        out: Pattern = []
        buf = ""
        i = 0
        n = len(text)
        while i < n:
            ch = text[i]
            if ch == "}":
                raise self.err(no, 'a } in text is written {"}"}')
            if ch != "{":
                buf += ch
                i += 1
                continue
            end = self.placeable_end(text, i, no)
            inner = text[i + 1:end]
            i = end + 1
            part = self.placeable(inner, no, in_variant)
            if isinstance(part, str):
                buf += part
            else:
                if buf:
                    out.append(Lit(buf))
                    buf = ""
                out.append(part)
        if buf:
            out.append(Lit(buf))
        return out

    def placeable_end(self, text: str, start: int, no: int) -> int:
        i = start + 1
        n = len(text)
        in_str = False
        while i < n:
            ch = text[i]
            if in_str:
                if ch == "\\" and i + 1 < n:
                    i += 2
                    continue
                if ch == '"':
                    in_str = False
            else:
                if ch == '"':
                    in_str = True
                elif ch == "{":
                    raise self.err(no, "a placeable inside a placeable is not allowed")
                elif ch == "}":
                    return i
            i += 1
        raise self.err(no, "unclosed placeable: the closing } is missing")

    def placeable(self, inner: str, no: int, in_variant: bool) -> Union[str, Part]:
        s = inner.strip(" ")
        if "->" in re.sub(r'"(?:\\.|[^"\\])*"', '""', s):
            if in_variant:
                raise self.err(no, "a select inside a variant (nested select) is not allowed")
            raise self.err(no, "one select per message, and it must be the whole value")
        if s.startswith('"'):
            return self.string_literal(s, no)
        m = re.match(rf"^\$({VAR})$", s)
        if m:
            return Var(m.group(1))
        m = re.match(rf"^NUMBER\(\s*\$({VAR})\s*\)$", s)
        if m:
            return Num(m.group(1))
        if s.startswith("NUMBER("):
            raise self.err(no, "NUMBER takes one variable and no options")
        if re.match(r"^[A-Z][A-Z0-9_-]*\(", s):
            raise self.err(no, f"function {s.split('(')[0]} is not allowed; only NUMBER($name)")
        m = re.match(r"^-brand-([a-z][a-z0-9]*(?:-[a-z][a-z0-9]*)*)$", s)
        if m:
            return Term(m.group(1))
        if s.startswith("-"):
            raise self.err(no, "only -brand-* terms may be used")
        if re.match(rf"^{IDENT}(\.[A-Za-z0-9_-]+)?$", s):
            raise self.err(no, f"a message reference ({s}) is not allowed; write the words out")
        if s.startswith("$"):
            raise self.err(no, f"{s} is not a variable name (lowercase words joined with _)")
        raise self.err(no, f"this placeable is not allowed: {{ {s} }}")

    def string_literal(self, s: str, no: int) -> str:
        m = re.match(r'^"((?:\\.|[^"\\])*)"$', s)
        if not m:
            raise self.err(no, 'a string literal is {"text"} and nothing else')
        raw = m.group(1)
        out = ""
        i = 0
        while i < len(raw):
            ch = raw[i]
            if ch != "\\":
                out += ch
                i += 1
                continue
            nxt = raw[i + 1]
            if nxt in '\\"':
                out += nxt
                i += 2
            elif nxt == "u" and re.match(r"^[0-9A-Fa-f]{4}$", raw[i + 2:i + 6]):
                out += chr(int(raw[i + 2:i + 6], 16))
                i += 6
            elif nxt == "U" and re.match(r"^[0-9A-Fa-f]{6}$", raw[i + 2:i + 8]):
                out += chr(int(raw[i + 2:i + 8], 16))
                i += 8
            else:
                raise self.err(no, f"unknown escape \\{nxt} in a string literal")
        return out
