from __future__ import annotations

import json
from typing import Dict, List

import check
import emit_arb
from check import Tree
from ftl import Lit, Message, Select, Term

CATS = ("zero", "one", "two", "few", "many", "other")


def spec(var_type: str) -> str:
    return "lld" if var_type == "num" else "@"


def unit(value: str) -> Dict[str, object]:
    return {"stringUnit": {"state": "translated", "value": value}}


def text_of(pattern, order: List[str], types: Dict[str, str], terms: Dict[str, str], bare: str = "", arg: str = "") -> str:
    out: List[str] = []
    for part in pattern:
        if isinstance(part, Lit):
            out.append(part.text.replace("%", "%%"))
        elif isinstance(part, Term):
            out.append(terms[part.name].replace("%", "%%"))
        else:
            kind = types.get(part.name, "str")
            if arg and part.name == arg:
                out.append("%arg")
            elif bare and part.name == bare:
                out.append("%" + spec(kind))
            else:
                out.append(f"%{order.index(part.name) + 1}${spec(kind)}")
    return "".join(out)


def localization(msg: Message, order: List[str], types: Dict[str, str], terms: Dict[str, str]) -> Dict[str, object]:
    if isinstance(msg.value, Select):
        sel = msg.value.var
        variants = sorted(msg.value.variants, key=lambda v: CATS.index(v.category))
        if len(order) == 1:
            plural = {v.category: unit(text_of(v.pattern, order, types, terms, bare=sel)) for v in variants}
            return {"variations": {"plural": plural}}
        plural = {v.category: unit(text_of(v.pattern, order, types, terms, arg=sel)) for v in variants}
        return {
            "stringUnit": {"state": "translated", "value": f"%#@{sel}@"},
            "substitutions": {
                sel: {
                    "argNum": order.index(sel) + 1,
                    "formatSpecifier": spec(types.get(sel, "num")),
                    "variations": {"plural": plural},
                }
            },
        }
    return unit(text_of(msg.value, order, types, terms))


def render(tree: Tree) -> str:
    cfg = tree.config
    en = check.english(tree)
    terms = tree.langs[cfg.source].files["shared.ftl"].terms if "shared.ftl" in tree.langs[cfg.source].files else {}
    strings: Dict[str, object] = {}
    for key in sorted(en):
        rel, en_msg = en[key]
        if "phone" not in check.targets(rel):
            continue
        order = check.var_order(en_msg)
        types = check.var_types_of(en_msg)
        entry: Dict[str, object] = {}
        if en_msg.notes:
            entry["comment"] = " ".join(en_msg.notes)
        entry["extractionState"] = "manual"
        locs: Dict[str, object] = {}
        for tag in [cfg.source] + [t for t in cfg.ship if t != cfg.source]:
            msg = en_msg if tag == cfg.source else emit_arb.phone_messages(tree, tag).get(key)
            if msg is None:
                continue
            locs[tag] = localization(msg, order, types, terms)
        entry["localizations"] = locs
        strings[key] = entry
    doc = {"sourceLanguage": cfg.source, "strings": strings, "version": "1.0"}
    text = json.dumps(doc, indent=2, ensure_ascii=False, separators=(",", " : "))
    return emit_arb.escape_hidden(text) + "\n"
