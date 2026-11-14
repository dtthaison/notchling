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
