from __future__ import annotations

import json
from typing import Dict, List, Optional

import check
import ftl
from check import Tree
from ftl import Lit, Message, Select, Term


def arb_name(key: str) -> str:
    first, *rest = key.split("-")
    return first + "".join(word.capitalize() for word in rest)


def arb_var(var: str) -> str:
    first, *rest = var.split("_")
    return first + "".join(word.capitalize() for word in rest)


def file_name(tag: str) -> str:
    return "app_" + tag.replace("-", "_") + ".arb"


def parent_of(tag: str) -> Optional[str]:
    if "-" not in tag:
        return None
    return tag.rsplit("-", 1)[0]


def icu_pattern(pattern, terms: Dict[str, str]) -> str:
    out: List[str] = []
    for part in pattern:
        if isinstance(part, Lit):
            out.append(part.text)
        elif isinstance(part, Term):
            out.append(terms[part.name])
        else:
            out.append("{" + arb_var(part.name) + "}")
    return "".join(out)


def icu(msg: Message, terms: Dict[str, str]) -> str:
    if isinstance(msg.value, Select):
        variants = sorted(msg.value.variants, key=lambda v: ("zero", "one", "two", "few", "many", "other").index(v.category))
        arms = " ".join(f"{v.category}{{{icu_pattern(v.pattern, terms)}}}" for v in variants)
        return "{" + arb_var(msg.value.var) + ", plural, " + arms + "}"
    return icu_pattern(msg.value, terms)


def escape_hidden(text: str) -> str:
    out: List[str] = []
    for ch in text:
        if ftl.hidden(ch) and ch != "\n":
            cp = ord(ch)
            if cp > 0xFFFF:
                cp -= 0x10000
                out.append("\\u%04x\\u%04x" % (0xD800 + (cp >> 10), 0xDC00 + (cp & 0x3FF)))
            else:
                out.append("\\u%04x" % cp)
        else:
            out.append(ch)
    return "".join(out)


def phone_messages(tree: Tree, tag: str) -> Dict[str, Message]:
    out: Dict[str, Message] = {}
    lang = tree.langs.get(tag)
    if lang is None:
        return out
    for rel, res in sorted(lang.files.items()):
        if "phone" not in check.targets(rel):
            continue
        for msg in res.messages:
            out[msg.key] = msg
    return out


def render(tree: Tree, tag: str) -> str:
    cfg = tree.config
    en = check.english(tree)
    en_lang = tree.langs[cfg.source]
    terms = en_lang.files["shared.ftl"].terms if "shared.ftl" in en_lang.files else {}
    chain: List[Dict[str, Message]] = []
    cur: Optional[str] = tag
    while cur is not None:
        if cur == tag or cur in cfg.ship:
            chain.append(phone_messages(tree, cur))
        cur = parent_of(cur)
    chain.append(phone_messages(tree, cfg.source))
    obj: Dict[str, object] = {"@@locale": tag.replace("-", "_")}
    for key in sorted(en):
        rel, en_msg = en[key]
        if "phone" not in check.targets(rel):
            continue
        chosen = en_msg
        for layer in chain:
            if key in layer:
                chosen = layer[key]
                break
        name = arb_name(key)
        obj[name] = icu(chosen, terms)
        meta: Dict[str, object] = {}
        if en_msg.notes:
            meta["description"] = " ".join(en_msg.notes)
        types = check.var_types_of(en_msg)
        if types:
            meta["placeholders"] = {
                arb_var(var): (
                    {"type": "int", "format": "decimalPattern"} if types[var] == "num" else {"type": "String"}
                )
                for var in check.var_order(en_msg)
            }
        if meta:
            obj["@" + name] = meta
    return escape_hidden(json.dumps(obj, indent=2, ensure_ascii=False)) + "\n"
