from __future__ import annotations

from dataclasses import dataclass
from typing import Dict, List, Optional

import check
from check import Tree

PARTS = ("shared", "consent", "mac", "phone")


@dataclass
class Row:
    tag: str
    by_part: Dict[str, float]
    all: float
    same_as_english: int


def part_of(rel: str) -> Optional[str]:
    if rel == "shared.ftl":
        return "shared"
    if rel == "consent.ftl":
        return "consent"
    if rel.startswith("mac/"):
        return "mac"
    if rel.startswith("phone/"):
        return "phone"
    return None


def percent(have: int, total: int) -> float:
    return 100.0 if total == 0 else 100.0 * have / total


def rows(tree: Tree, lang: Optional[str] = None) -> List[Row]:
    en = check.english(tree)
    totals = {p: 0 for p in PARTS}
    for key, (rel, _) in en.items():
        part = part_of(rel)
        if part:
            totals[part] += 1
    tags = [tree.config.source] + [t for t in check.listed(tree.config) if t != tree.config.source]
    out: List[Row] = []
    for tag in tags:
        if lang is not None and tag != lang:
            continue
        have = {p: 0 for p in PARTS}
        same = 0
        found = tree.langs.get(tag)
        if found is not None:
            seen = set()
            for rel, res in found.files.items():
                for msg in res.messages:
                    if msg.key in seen or msg.key not in en:
                        continue
                    seen.add(msg.key)
                    part = part_of(en[msg.key][0])
                    if part:
                        have[part] += 1
                    if tag != tree.config.source and msg.value == en[msg.key][1].value:
                        same += 1
        by_part = {p: percent(have[p], totals[p]) for p in PARTS}
        out.append(Row(tag, by_part, percent(sum(have.values()), sum(totals.values())), same))
    return out


def pct(value: float) -> str:
    return "100%" if value >= 100.0 else f"{int(value)}%"


def text(items: List[Row]) -> str:
    lines = [f"{'Language':<10}{'shared':<8}{'consent':<9}{'mac':<7}{'phone':<7}{'all':<7}same as English"]
    for row in items:
        cells = "".join(f"{pct(row.by_part[p]):<{w}}" for p, w in zip(PARTS, (8, 9, 7, 7)))
        lines.append(f"{row.tag:<10}{cells}{pct(row.all):<7}{row.same_as_english}")
    return "\n".join(lines) + "\n"


def markdown(items: List[Row]) -> str:
    lines = [
        "| Language | shared | consent | mac | phone | all | same as English |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    for row in items:
        cells = " | ".join(pct(row.by_part[p]) for p in PARTS)
        lines.append(f"| {row.tag} | {cells} | {pct(row.all)} | {row.same_as_english} |")
    return "\n".join(lines) + "\n"


def missing(tree: Tree, tag: str) -> List[str]:
    en = check.english(tree)
    found = tree.langs.get(tag)
    have = set()
    if found is not None:
        for res in found.files.values():
            have.update(m.key for m in res.messages)
    return sorted(key for key in en if key not in have)
