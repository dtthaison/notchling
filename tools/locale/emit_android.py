from __future__ import annotations

import re
from typing import Dict, List

import check
import emit_arb
from check import Tree
from ftl import Lit, Message, Select, Term

CATS = ("zero", "one", "two", "few", "many", "other")


def qualifier(tag: str) -> str:
    parts = tag.split("-")
    script = [p for p in parts[1:] if len(p) == 4 and p.isalpha()]
    if script:
        return "values-b+" + "+".join(parts)
    if len(parts) == 2:
        return f"values-{parts[0]}-r{parts[1]}"
    return f"values-{parts[0]}"


def xml_escape(text: str, formatted: bool) -> str:
    out = text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
    out = out.replace("\\", "\\\\").replace("'", "\\'").replace('"', '\\"')
    if formatted:
        out = out.replace("%", "%%")
    return out


def edges(text: str) -> str:
    if text[:1] in ("@", "?"):
        text = "\\" + text
    if text.startswith(" "):
        text = "\\u0020" + text[1:]
    if text.endswith(" "):
        text = text[:-1] + "\\u0020"
    return text


def text_of(pattern, order: List[str], types: Dict[str, str], terms: Dict[str, str], formatted: bool) -> str:
    out: List[str] = []
    for part in pattern:
        if isinstance(part, Lit):
            out.append(xml_escape(part.text, formatted))
        elif isinstance(part, Term):
            out.append(xml_escape(terms[part.name], formatted))
        else:
            kind = "d" if types.get(part.name) == "num" else "s"
            out.append(f"%{order.index(part.name) + 1}${kind}")
    return edges("".join(out))


def comment(notes: List[str]) -> str:
    return "    <!-- " + re.sub(r"-{2,}", "-", " ".join(notes)).replace("&", "&amp;").replace("<", "&lt;") + " -->\n"


def element(key: str, msg: Message, en_msg: Message, terms: Dict[str, str]) -> str:
    order = check.var_order(en_msg)
    types = check.var_types_of(en_msg)
    name = key.replace("-", "_")
    formatted = bool(order)
    out = comment(en_msg.notes) if en_msg.notes else ""
    if isinstance(msg.value, Select):
        variants = sorted(msg.value.variants, key=lambda v: CATS.index(v.category))
        out += f'    <plurals name="{name}">\n'
        for v in variants:
            out += f'        <item quantity="{v.category}">{text_of(v.pattern, order, types, terms, True)}</item>\n'
        out += "    </plurals>\n"
    else:
        out += f'    <string name="{name}">{text_of(msg.value, order, types, terms, formatted)}</string>\n'
    return out


def render(tree: Tree) -> Dict[str, str]:
    cfg = tree.config
    en = check.english(tree)
    terms = tree.langs[cfg.source].files["shared.ftl"].terms if "shared.ftl" in tree.langs[cfg.source].files else {}
    files: Dict[str, str] = {}
    for tag in [cfg.source] + [t for t in cfg.ship if t != cfg.source]:
        if tag not in tree.langs:
            continue
        have = {key: en[key][1] for key in en if "phone" in check.targets(en[key][0])} if tag == cfg.source else emit_arb.phone_messages(tree, tag)
        body = ""
        for key in sorted(have):
            if key not in en or "phone" not in check.targets(en[key][0]):
                continue
            body += element(key, have[key], en[key][1], terms)
        if not body:
            continue
        folder = "values" if tag == cfg.source else qualifier(tag)
        files[f"{folder}/strings.xml"] = '<?xml version="1.0" encoding="utf-8"?>\n<resources>\n' + body + "</resources>\n"
    return files
