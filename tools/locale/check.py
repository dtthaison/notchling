from __future__ import annotations

import json
import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Dict, FrozenSet, Iterator, List, Tuple

import ftl
from ftl import FtlError, Lit, Message, Resource, Select, Term

RULE_SUBSET = "GUARD-LOCALE-SUBSET"
RULE_MATCH = "GUARD-LOCALE-MATCH"
SHARED_NAMESPACES = ("language", "state", "session", "item")
OUTPUTS = ("rust", "arb")
KEY_RE = re.compile(r"^[a-z][a-z0-9]*(-[a-z][a-z0-9]*)+$")
TAG_RE = re.compile(r"^[a-z]{2,3}(-[A-Z][a-z]{3})?(-([A-Z]{2}|[0-9]{3}))?$")
FILE_RE = re.compile(r"^(shared\.ftl|consent\.ftl|(mac|phone)/[a-z][a-z0-9-]*\.ftl)$")


@dataclass
class Config:
    source: str
    ship: List[str]
    draft: List[str]
    outputs: List[str]


@dataclass
class Lang:
    tag: str
    files: Dict[str, Resource] = field(default_factory=dict)


@dataclass
class Tree:
    root: Path
    config: Config
    langs: Dict[str, Lang]


@dataclass
class Problem:
    rule: str
    path: str
    line: int
    message: str

    def __str__(self) -> str:
        return f"{self.rule}: {self.path}:{self.line}: {self.message}"


def problem_of(err: FtlError) -> Problem:
    return Problem(err.rule, err.path, err.line, err.message)


def display(root: Path, *rest: str) -> str:
    return "/".join((root.name,) + rest)


def load(root: Path) -> Tuple[Tree, List[Problem]]:
    problems: List[Problem] = []
    config = Config("en", ["en"], [], list(OUTPUTS))
    cfg_path = root / "languages.json"
    cfg_disp = display(root, "languages.json")
    if not cfg_path.is_file():
        problems.append(Problem(RULE_SUBSET, cfg_disp, 1, "languages.json is missing"))
    else:
        try:
            raw = json.loads(cfg_path.read_text(encoding="utf-8"))
        except ValueError as exc:
            raw = None
            problems.append(Problem(RULE_SUBSET, cfg_disp, 1, f"languages.json is not valid JSON: {exc}"))
        if isinstance(raw, dict):
            config = Config(
                raw.get("source", "en"),
                list(raw.get("ship", [])),
                list(raw.get("draft", [])),
                list(raw.get("outputs", list(OUTPUTS))),
            )
        elif raw is not None:
            problems.append(Problem(RULE_SUBSET, cfg_disp, 1, "languages.json must be an object"))
    langs: Dict[str, Lang] = {}
    folders = sorted(p for p in root.iterdir() if p.is_dir() and not p.name.startswith(".")) if root.is_dir() else []
    for folder in folders:
        lang = Lang(folder.name)
        for file in sorted(folder.rglob("*.ftl")):
            rel = file.relative_to(folder).as_posix()
            disp = display(root, folder.name, rel)
            try:
                lang.files[rel] = ftl.parse(file.read_text(encoding="utf-8"), disp)
            except FtlError as exc:
                problems.append(problem_of(exc))
            except UnicodeDecodeError:
                problems.append(Problem(RULE_SUBSET, disp, 1, "the file is not UTF-8"))
        langs[folder.name] = lang
    return Tree(root, config, langs), problems


def targets(rel: str) -> FrozenSet[str]:
    if rel in ("shared.ftl", "consent.ftl"):
        return frozenset({"rust", "phone"})
    if rel.startswith("mac/"):
        return frozenset({"rust"})
    if rel.startswith("phone/"):
        return frozenset({"phone"})
    return frozenset()


def file_stem(rel: str) -> str:
    return rel.rsplit("/", 1)[-1][: -len(".ftl")]


def iter_parts(value) -> Iterator:
    if isinstance(value, Select):
        for variant in value.variants:
            for part in variant.pattern:
                yield part
    else:
        for part in value:
            yield part


def listed(config: Config) -> List[str]:
    return list(config.ship) + [t for t in config.draft if t not in config.ship]


def check_structure(tree: Tree) -> List[Problem]:
    out: List[Problem] = []
    root = tree.root
    cfg = tree.config
    cfg_disp = display(root, "languages.json")

    for name in cfg.outputs:
        if name not in OUTPUTS:
            out.append(Problem(RULE_SUBSET, cfg_disp, 1, f'unknown output "{name}"; use "rust" or "arb"'))
    if cfg.source != "en":
        out.append(Problem(RULE_SUBSET, cfg_disp, 1, 'the source language is "en"'))
    if "en" not in cfg.ship:
        out.append(Problem(RULE_SUBSET, cfg_disp, 1, '"en" must be in ship'))
    for tag in cfg.ship:
        if tag in cfg.draft:
            out.append(Problem(RULE_SUBSET, cfg_disp, 1, f"{tag} is in both ship and draft"))
    for tag in listed(cfg):
        if tag not in tree.langs:
            if not TAG_RE.match(tag):
                out.append(Problem(RULE_SUBSET, cfg_disp, 1, f"language tag {tag} is not in the form en, pt-BR, zh-Hans"))
            out.append(Problem(RULE_SUBSET, cfg_disp, 1, f"{tag} is listed but has no folder in {root.name}/"))

    known = set(listed(cfg))
    for tag, lang in sorted(tree.langs.items()):
        folder_disp = display(root, tag)
        if not TAG_RE.match(tag):
            out.append(
                Problem(RULE_SUBSET, folder_disp, 1, f"folder name {tag} is not a language tag in the form en, pt-BR, zh-Hans")
            )
        if tag not in known:
            out.append(
                Problem(RULE_SUBSET, folder_disp, 1, f"{tag} is not listed in languages.json (ship or draft)")
            )

    english_terms: Dict[str, str] = {}
    en = tree.langs.get("en")
    if en and "shared.ftl" in en.files:
        english_terms = en.files["shared.ftl"].terms

    seen: Dict[Tuple[str, str], Tuple[str, int]] = {}
    for tag, lang in sorted(tree.langs.items()):
        for rel, res in sorted(lang.files.items()):
            if not FILE_RE.match(rel):
                out.append(
                    Problem(
                        RULE_SUBSET,
                        res.path,
                        1,
                        "unknown file; a language has shared.ftl, consent.ftl, mac/<area>.ftl and phone/<feature>.ftl",
                    )
                )
                continue
            stem = file_stem(rel)
            is_area = rel not in ("shared.ftl", "consent.ftl")
            if is_area and (stem in SHARED_NAMESPACES or stem == "consent"):
                out.append(Problem(RULE_SUBSET, res.path, 1, f'"{stem}" is a reserved name; pick another file name'))
            if res.terms and not (tag == "en" and rel == "shared.ftl"):
                for name, line in sorted(res.term_lines.items()):
                    out.append(
                        Problem(
                            RULE_SUBSET,
                            res.path,
                            line,
                            "brand terms (-brand-*) are defined only in the English shared.ftl",
                        )
                    )
            for msg in res.messages:
                out.extend(check_key(res, rel, stem, is_area, msg))
                ident = (tag, msg.key)
                if ident in seen:
                    other_path, other_line = seen[ident]
                    out.append(
                        Problem(
                            RULE_SUBSET,
                            res.path,
                            msg.line,
                            f"key {msg.key} is also defined at {other_path}:{other_line}",
                        )
                    )
                    out.append(
                        Problem(
                            RULE_SUBSET,
                            other_path,
                            other_line,
                            f"key {msg.key} is also defined at {res.path}:{msg.line}",
                        )
                    )
                else:
                    seen[ident] = (res.path, msg.line)
                for part in iter_parts(msg.value):
                    if isinstance(part, Lit) and ("{" in part.text or "}" in part.text) and not rel.startswith("mac/"):
                        out.append(
                            Problem(
                                RULE_SUBSET,
                                res.path,
                                msg.line,
                                "a { or } in text is allowed only in mac/ files (the phone's ICU has no safe quoting)",
                            )
                        )
                        break
                for part in iter_parts(msg.value):
                    if isinstance(part, Term) and part.name not in english_terms:
                        out.append(
                            Problem(
                                RULE_SUBSET,
                                res.path,
                                msg.line,
                                f"unknown brand term -brand-{part.name}; define it in the English shared.ftl",
                            )
                        )
                        break

    for tag in listed(cfg):
        lang = tree.langs.get(tag)
        if lang is None:
            continue
        shared = lang.files.get("shared.ftl")
        has = shared is not None and any(m.key == "language-name" for m in shared.messages)
        if not has:
            path = shared.path if shared else display(root, tag, "shared.ftl")
            out.append(Problem(RULE_SUBSET, path, 1, "shared.ftl must define language-name, the language's own name"))
    return out


def check_key(res: Resource, rel: str, stem: str, is_area: bool, msg: Message) -> List[Problem]:
    out: List[Problem] = []
    key = msg.key
    if not KEY_RE.match(key) or len(key) > 64:
        out.append(
            Problem(
                RULE_SUBSET,
                res.path,
                msg.line,
                f"key {key} is not lowercase words joined by - (at least two, at most 64 characters, each starting with a letter)",
            )
        )
        return out
    first = key.split("-", 1)[0]
    if rel == "shared.ftl":
        if first not in SHARED_NAMESPACES:
            out.append(
                Problem(
                    RULE_SUBSET,
                    res.path,
                    msg.line,
                    f"shared.ftl keys start with {', '.join(n + '-' for n in SHARED_NAMESPACES)}; {key} belongs in an area file",
                )
            )
    elif not key.startswith(stem + "-"):
        out.append(Problem(RULE_SUBSET, res.path, msg.line, f"a key in {rel} starts with {stem}-; {key} does not"))
    return out


def english(tree: Tree) -> Dict[str, Tuple[str, Message]]:
    en = tree.langs.get(tree.config.source)
    out: Dict[str, Tuple[str, Message]] = {}
    if en is None:
        return out
    for rel, res in sorted(en.files.items()):
        for msg in res.messages:
            out.setdefault(msg.key, (rel, msg))
    return out


def var_types_of(msg: Message) -> Dict[str, str]:
    types: Dict[str, str] = {}
    if isinstance(msg.value, Select):
        types[msg.value.var] = "num"
    for part in iter_parts(msg.value):
        if isinstance(part, ftl.Num):
            types[part.name] = "num"
        elif isinstance(part, ftl.Var):
            types.setdefault(part.name, "str")
    return types


def var_types(tree: Tree) -> Dict[str, Dict[str, str]]:
    return {key: var_types_of(msg) for key, (_, msg) in english(tree).items()}


def var_order(msg: Message) -> List[str]:
    order: List[str] = []
    if isinstance(msg.value, Select):
        order.append(msg.value.var)
    for part in iter_parts(msg.value):
        if isinstance(part, (ftl.Var, ftl.Num)) and part.name not in order:
            order.append(part.name)
    return order


LINK_RES = (
    (re.compile(r"https?://|ftp://", re.I), "a link"),
    (re.compile(r"\bwww\.", re.I), "a link"),
    (re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+\.[A-Za-z]{2,}"), "an address"),
    (re.compile(r"</?[A-Za-z][^>]*>"), "markup"),
)


def measure(value, terms: Dict[str, str]) -> int:
    def one(pattern) -> int:
        total = 0
        for part in pattern:
            if isinstance(part, Lit):
                total += len(part.text)
            elif isinstance(part, Term):
                total += len(terms.get(part.name, ""))
            else:
                total += 1
        return total

    if isinstance(value, Select):
        return max(one(v.pattern) for v in value.variants)
    return one(value)


def check_match(tree: Tree) -> List[Problem]:
    out: List[Problem] = []
    en_msgs = english(tree)
    source = tree.config.source
    en = tree.langs.get(source)
    terms = en.files["shared.ftl"].terms if en and "shared.ftl" in en.files else {}
    for tag, lang in sorted(tree.langs.items()):
        if tag == source:
            continue
        for rel, res in sorted(lang.files.items()):
            for msg in res.messages:
                out.extend(match_one(res.path, rel, msg, en_msgs, terms))
    return out


def match_one(path: str, rel: str, msg: Message, en_msgs, terms: Dict[str, str]) -> List[Problem]:
    def bad(message: str) -> List[Problem]:
        return [Problem(RULE_MATCH, path, msg.line, message)]

    if msg.key not in en_msgs:
        return bad(
            f"key {msg.key} is not in English; if English renamed it, run python3 tools/locale/gen.py --prune in the same pull request"
        )
    en_rel, en_msg = en_msgs[msg.key]
    if en_rel != rel:
        return bad(f"key {msg.key} belongs in {en_rel}, where English has it")
    types = var_types_of(en_msg)
    problems: List[Problem] = []
    if isinstance(msg.value, Select):
        sel = msg.value.var
        if sel not in types:
            problems += bad(f"the select is over ${sel}, which English does not have")
        elif types[sel] != "num":
            problems += bad(f"the select is over ${sel}, a string variable in English; only numbers can select")
    for part in iter_parts(msg.value):
        if isinstance(part, (ftl.Var, ftl.Num)):
            if part.name not in types:
                problems += bad(f"${part.name} is not a variable of the English message")
            elif isinstance(part, ftl.Num) and types[part.name] != "num":
                problems += bad(f"${part.name} is a string in English; NUMBER() takes numbers only")
    if problems:
        return problems
    required = set(types)
    if isinstance(msg.value, Select):
        other = [v for v in msg.value.variants if v.default][0]
        present = {p.name for p in other.pattern if isinstance(p, (ftl.Var, ftl.Num))}
    else:
        present = {p.name for p in msg.value if isinstance(p, (ftl.Var, ftl.Num))}
    for name in sorted(required - present):
        where = "the *[other] variant" if isinstance(msg.value, Select) else "the message"
        problems += bad(f"${name} is in the English message and must be in {where}")
    for part in iter_parts(msg.value):
        if isinstance(part, Lit):
            for regex, what in LINK_RES:
                if regex.search(part.text):
                    problems += bad(f"a translation carries {what}, which English does not have here")
                    break
    cap = 3 * measure(en_msg.value, terms) + 20
    size = measure(msg.value, terms)
    if size > cap:
        problems += bad(f"the text is {size} characters; the most allowed is {cap} (three times English plus 20)")
    return problems
