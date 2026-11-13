#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

ROOT = Path(__file__).resolve().parents[2]
SCHEMA = ROOT / "protocol/agentconsole.v1.schema.json"
RUST_OUT = ROOT / "crates/protocol/src/generated.rs"
DART_OUT = ROOT / "apps/flutter/lib/core/protocol/generated.dart"
RULE = "GUARD-PROTOCOL-GEN"

ANNOTATIONS = {"title", "description", "default", "$comment", "$schema", "$id", "version", "$defs"}
KEYWORDS = {"type", "const", "enum", "properties", "required", "additionalProperties", "items", "$ref", "oneOf", "anyOf"}
RUST_KEYWORDS = {
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static",
    "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while", "abstract", "become", "box", "do",
    "final", "macro", "override", "priv", "try", "typeof", "unsized", "virtual", "yield",
}
DART_KEYWORDS = {
    "abstract", "as", "assert", "async", "await", "break", "case", "catch", "class", "const", "continue", "default", "deferred",
    "do", "dynamic", "else", "enum", "export", "extends", "extension", "external", "factory", "false", "final", "finally",
    "for", "get", "if", "implements", "import", "in", "interface", "is", "late", "library", "mixin", "new", "null", "on",
    "operator", "part", "required", "rethrow", "return", "set", "static", "super", "switch", "sync", "this", "throw", "true",
    "try", "typedef", "var", "void", "while", "with", "yield", "toJson", "hashCode", "runtimeType",
}
SCALARS = ("string", "integer", "number", "boolean")

Type = Tuple[Any, ...]


class SchemaError(ValueError):
    pass


class Field:
    def __init__(self, wire: str, type_: Type, doc: str, required: bool, const: Any, default: Any, has_default: bool):
        self.wire = wire
        self.type = type_
        self.doc = doc
        self.required = required or const is not None
        self.const = const
        self.default = default
        self.has_default = has_default


class Struct:
    def __init__(self, name: str, doc: str, fields: List[Field], children: List["Struct"]):
        self.name = name
        self.doc = doc
        self.fields = fields
        self.children = children


class Model:

    def __init__(self, schema: Dict[str, Any]):
        defs = schema.get("$defs")
        if not isinstance(defs, dict):
            raise SchemaError("the schema has no $defs")
        self.defs = defs
        self.kind: Dict[str, str] = {}
        for name, definition in defs.items():
            self.kind[name] = self._classify(name, definition)
        self.items: List[Tuple[str, Any]] = []
        self.exported: List[str] = []
        for name, definition in defs.items():
            kind = self.kind[name]
            doc = _doc(definition)
            if kind == "enum":
                values = definition["enum"]
                if not values or not all(isinstance(v, str) for v in values):
                    raise SchemaError(f"{name}: an enum must hold strings")
                self.items.append(("enum", (name, doc, list(values))))
            elif kind == "struct":
                self.items.append(("struct", self._struct(name, definition)))
                self.exported.append(name)
            elif kind == "alias":
                target = _ref_name(definition["$ref"], name)
                if target not in defs:
                    raise SchemaError(f"{name}: $ref to {target}, which is not defined")
                if self.kind[target] in ("union", "any"):
                    raise SchemaError(f"{name}: an alias of {target}, which is not a type")
                self.items.append(("alias", (name, doc, target)))
                self.exported.append(name)

    def _classify(self, name: str, definition: Dict[str, Any]) -> str:
        _keywords(definition, name)
        if "$ref" in definition:
            return "alias"
        if definition.get("type") == "string" and "enum" in definition:
            return "enum"
        if definition.get("type") == "object":
            return "struct"
        options = definition.get("oneOf", definition.get("anyOf"))
        if isinstance(options, list) and options:
            if all(isinstance(o, dict) and "$ref" in o for o in options):
                return "union"
            return "any"
        raise SchemaError(f"{name}: not an object, enum, alias or union")

    def _struct(self, name: str, definition: Dict[str, Any]) -> Struct:
        properties = definition.get("properties", {})
        required = definition.get("required", [])
        for wanted in required:
            if wanted not in properties:
                raise SchemaError(f"{name}: required field {wanted!r} is not a property")
        fields: List[Field] = []
        children: List[Struct] = []
        for wire, prop in properties.items():
            if not re.match(r"^[A-Za-z][A-Za-z0-9]*$", wire):
                raise SchemaError(f"{name}.{wire}: field names are letters and digits")
            type_ = self._type(prop, f"{name}.{wire}", name + _pascal(wire), children)
            const = prop.get("const")
            if const is not None and not isinstance(const, (str, int, bool)):
                raise SchemaError(f"{name}.{wire}: const must be a string, integer or boolean")
            has_default = "default" in prop
            if has_default and (wire in required or const is not None):
                raise SchemaError(f"{name}.{wire}: a required field has no default")
            fields.append(Field(wire, type_, _doc(prop), wire in required, const, prop.get("default"), has_default))
        return Struct(name, _doc(definition), fields, children)

    def _type(self, prop: Dict[str, Any], where: str, inline_name: str, children: List[Struct]) -> Type:
        _keywords(prop, where)
        if "$ref" in prop:
            target = _ref_name(prop["$ref"], where)
            if target not in self.defs:
                raise SchemaError(f"{where}: $ref to {target}, which is not defined")
            kind = self.kind[target]
            if kind == "any":
                return ("any",)
            if kind == "union":
                raise SchemaError(f"{where}: a field cannot be the union {target}")
            return ("ref", target)
        kind = prop.get("type")
        if kind in SCALARS:
            return (kind,)
        if kind == "array":
            items = prop.get("items")
            if items is None:
                return ("array", ("any",))
            return ("array", self._type(items, where + "[]", inline_name + "Item", children))
        if kind == "object":
            if "properties" not in prop:
                return ("any",)
            child = self._struct(inline_name, prop)
            children.append(child)
            return ("ref", inline_name)
        if kind is None or isinstance(kind, list):
            return ("any",)
        raise SchemaError(f"{where}: unsupported type {kind!r}")

    def is_enum(self, name: str) -> bool:
        return self.kind.get(name) == "enum"


def _keywords(node: Dict[str, Any], where: str) -> None:
    if not isinstance(node, dict):
        raise SchemaError(f"{where}: a schema must be an object")
    for key in node:
        if key not in KEYWORDS and key not in ANNOTATIONS:
            raise SchemaError(f"{where}: unsupported keyword {key!r}")


def _ref_name(ref: str, where: str) -> str:
    if not ref.startswith("#/$defs/"):
        raise SchemaError(f"{where}: only #/$defs/ references are supported, got {ref!r}")
    return ref[len("#/$defs/"):]


def _doc(node: Dict[str, Any]) -> str:
    text = node.get("description", "")
    return text.strip() if isinstance(text, str) else ""


def _pascal(text: str) -> str:
    return "".join(part[:1].upper() + part[1:] for part in re.split(r"[^A-Za-z0-9]+", text) if part)


def _lower_camel(text: str) -> str:
    pascal = _pascal(text)
    return pascal[:1].lower() + pascal[1:]


def _snake(wire: str) -> str:
    return re.sub(r"(?<=[a-z0-9])(?=[A-Z])", "_", wire).lower()


def _camel_of_snake(ident: str) -> str:
    first, *rest = ident.split("_")
    return first + "".join(part[:1].upper() + part[1:] for part in rest)


def _docs(doc: str, indent: str) -> List[str]:
    return [f"{indent}/// {line}".rstrip() for line in doc.splitlines()] if doc else []


def _rust_type(t: Type) -> str:
    kind = t[0]
    return {"string": "String", "integer": "i64", "number": "f64", "boolean": "bool", "any": "Value"}.get(kind) or (
        f"Vec<{_rust_type(t[1])}>" if kind == "array" else t[1])


def _rust_literal(t: Type, value: Any, where: str) -> Optional[str]:
    kind = t[0]
    if kind == "string" and isinstance(value, str):
        return None if value == "" else json.dumps(value) + ".to_string()"
    if kind == "integer" and isinstance(value, int) and not isinstance(value, bool):
        return None if value == 0 else str(value)
    if kind == "number" and isinstance(value, (int, float)) and not isinstance(value, bool):
        return None if value == 0 else repr(float(value))
    if kind == "boolean" and isinstance(value, bool):
        return "true" if value else None
    if kind == "array" and value == []:
        return None
    raise SchemaError(f"{where}: unsupported default {value!r}")


def _rust_struct(model: Model, struct: Struct) -> List[str]:
    camel = any(f.wire != f.wire.lower() for f in struct.fields)
    out = _docs(struct.doc, "")
    out.append("#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]")
    if camel:
        out.append('#[serde(rename_all = "camelCase")]')
    if not struct.fields:
        return out + [f"pub struct {struct.name} {{}}"]
    out.append(f"pub struct {struct.name} {{")
    helpers: List[str] = []
    for f in struct.fields:
        snake = _snake(f.wire)
        ident = "r#" + snake if snake in RUST_KEYWORDS else snake
        default_wire = _camel_of_snake(snake) if camel else snake
        parts: List[str] = []
        type_ = _rust_type(f.type)
        if f.required:
            if f.const is not None:
                type_ = {str: "String", bool: "bool", int: "i64"}[type(f.const)]
        elif f.has_default:
            literal = _rust_literal(f.type, f.default, f"{struct.name}.{f.wire}")
            if literal is None:
                parts.append("default")
            else:
                fn = f"default_{_snake(struct.name)}_{snake}"
                parts.append(f'default = "{fn}"')
                helpers += ["", f"fn {fn}() -> {type_} {{", f"    {literal}", "}"]
        else:
            parts += ["default", 'skip_serializing_if = "Option::is_none"']
            type_ = f"Option<{type_}>"
        if f.wire != default_wire:
            parts.append(f'rename = "{f.wire}"')
        out += _docs(f.doc, "    ")
        if parts:
            out.append(f"    #[serde({', '.join(parts)})]")
        out.append(f"    pub {ident}: {type_},")
    out.append("}")
    return out + helpers


def _rust_enum(name: str, doc: str, values: List[str]) -> List[str]:
    out = _docs(doc, "")
    out.append("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]")
    out.append(f"pub enum {name} {{")
    seen = {"Unknown"}
    for value in values:
        variant = _pascal(value)
        if not variant or variant[0].isdigit() or variant in seen:
            raise SchemaError(f"{name}: {value!r} cannot be a variant name")
        seen.add(variant)
        out += [f'    #[serde(rename = "{value}")]', f"    {variant},"]
    out += ["    #[serde(other)]", "    Unknown,", "}"]
    return out


def rust_types(schema: Dict[str, Any]) -> str:
    model = Model(schema)
    blocks: List[str] = []

    def struct(s: Struct) -> None:
        blocks.append("\n".join(_rust_struct(model, s)) + "\n")
        for child in s.children:
            struct(child)

    for kind, item in model.items:
        if kind == "enum":
            blocks.append("\n".join(_rust_enum(*item)) + "\n")
        elif kind == "struct":
            struct(item)
        else:
            name, doc, target = item
            blocks.append("\n".join(_docs(doc, "") + [f"pub type {name} = {target};"]) + "\n")
    return "\n".join(blocks)


def rust_registry(schema: Dict[str, Any]) -> str:
    model = Model(schema)
    arms = "".join(f'        "{name}" => round::<{name}>(value),\n' for name in model.exported)
    return (
        "/// Parses `value` as the type named `name` and writes it back. `None` when the schema has no such type.\n"
        "pub fn roundtrip(name: &str, value: &Value) -> Option<Result<Value, serde_json::Error>> {\n"
        "    Some(match name {\n"
        f"{arms}"
        "        _ => return None,\n"
        "    })\n"
        "}\n"
    )


RUST_HEAD = """\
//! The `agentconsole.v1` wire types, generated from `protocol/agentconsole.v1.schema.json` by
//! `tools/protocol/gen.py`. @generated: do not edit by hand; change the schema and run the generator.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

"""

RUST_ROUND = """\
fn round<T: DeserializeOwned + Serialize>(value: &Value) -> Result<Value, serde_json::Error> {
    let typed: T = serde_json::from_value(value.clone())?;
    serde_json::to_value(typed)
}
"""


def render_rust(schema: Dict[str, Any]) -> str:
    return RUST_HEAD + rust_types(schema) + "\n" + RUST_ROUND + "\n" + rust_registry(schema)


def _dart_type(t: Type) -> str:
    kind = t[0]
    return {"string": "String", "integer": "int", "number": "double", "boolean": "bool", "any": "Object?"}.get(kind) or (
        f"List<{_dart_type(t[1])}>" if kind == "array" else t[1])


def _dart_string(value: str) -> str:
    return "'" + value.replace("\\", "\\\\").replace("'", "\\'").replace("$", "\\$").replace("\n", "\\n") + "'"


def _dart_reader(model: Model, t: Type) -> str:
    kind = t[0]
    if kind in SCALARS:
        return {"string": "_asString", "integer": "_asInt", "number": "_asDouble", "boolean": "_asBool"}[kind]
    if kind == "any":
        return "_asAny"
    if kind == "array":
        return f"(Object? v) => _asList<{_dart_type(t[1])}>(v, {_dart_reader_inner(model, t[1])})"
    return _dart_reader_inner(model, t)


def _dart_reader_inner(model: Model, t: Type) -> str:
    kind = t[0]
    if kind == "ref":
        if model.is_enum(t[1]):
            return f"(Object? e) => {t[1]}.parse(_asString(e))"
        return f"(Object? e) => {t[1]}.fromJson(_asMap(e))"
    if kind == "array":
        return f"(Object? e) => _asList<{_dart_type(t[1])}>(e, {_dart_reader_inner(model, t[1])})"
    return _dart_reader(model, t)


def _dart_field_reader(model: Model, t: Type) -> str:
    if t[0] == "ref":
        if model.is_enum(t[1]):
            return f"(Object? v) => {t[1]}.parse(_asString(v))"
        return f"(Object? v) => {t[1]}.fromJson(_asMap(v))"
    return _dart_reader(model, t)


def _dart_writer(model: Model, t: Type, expr: str, optional: bool) -> str:
    bang = "!" if optional else ""
    kind = t[0]
    if kind == "ref":
        return f"{expr}{bang}.wire" if model.is_enum(t[1]) else f"{expr}{bang}.toJson()"
    if kind == "array":
        inner = t[1]
        if _plain(model, inner):
            return expr
        element = _dart_writer(model, inner, "e", False)
        return f"<Object?>[for (final {_dart_type(inner)} e in {expr}{bang}) {element}]"
    return expr


def _plain(model: Model, t: Type) -> bool:
    if t[0] == "ref":
        return False
    if t[0] == "array":
        return _plain(model, t[1])
    return True


def _dart_default(t: Type, value: Any, where: str) -> str:
    kind = t[0]
    if kind == "string" and isinstance(value, str):
        return _dart_string(value)
    if kind == "integer" and isinstance(value, int) and not isinstance(value, bool):
        return str(value)
    if kind == "number" and isinstance(value, (int, float)) and not isinstance(value, bool):
        return repr(float(value))
    if kind == "boolean" and isinstance(value, bool):
        return "true" if value else "false"
    if kind == "array" and value == []:
        return f"const <{_dart_type(t[1])}>[]"
    raise SchemaError(f"{where}: unsupported default {value!r}")


def _dart_const(value: Any) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    return _dart_string(value) if isinstance(value, str) else str(value)


def _dart_struct(model: Model, struct: Struct) -> List[str]:
    out = _docs(struct.doc, "")
    out.append(f"class {struct.name} {{")
    out.append(f"  const {struct.name}(" + ("{" if struct.fields else ");"))
    for f in struct.fields:
        _dart_name(struct.name, f.wire)
        if f.const is not None:
            out.append(f"    this.{f.wire} = {_dart_const(f.const)},")
        elif f.required:
            out.append(f"    required this.{f.wire},")
        elif f.has_default:
            out.append(f"    this.{f.wire} = {_dart_default(f.type, f.default, struct.name + '.' + f.wire)},")
        else:
            out.append(f"    this.{f.wire},")
    if struct.fields:
        out.append("  });")
    out.append("")
    out.append(f"  factory {struct.name}.fromJson(JsonMap json) => {struct.name}(")
    for f in struct.fields:
        reader = _dart_field_reader(model, f.type)
        if f.required:
            out.append(f"    {f.wire}: _need(json, '{f.wire}', {reader}),")
        elif f.has_default:
            default = _dart_default(f.type, f.default, struct.name + "." + f.wire)
            out.append(f"    {f.wire}: _maybe(json, '{f.wire}', {reader}) ?? {default},")
        elif f.type == ("any",):
            out.append(f"    {f.wire}: json['{f.wire}'],")
        else:
            out.append(f"    {f.wire}: _maybe(json, '{f.wire}', {reader}),")
    out.append("  );")
    out.append("")
    out.append("  JsonMap toJson() => <String, Object?>{")
    for f in struct.fields:
        optional = not f.required and not f.has_default
        value = _dart_writer(model, f.type, f.wire, optional)
        out.append(f"    if ({f.wire} != null) '{f.wire}': {value}," if optional else f"    '{f.wire}': {value},")
    out.append("  };")
    for index, f in enumerate(struct.fields):
        if index == 0 or f.doc:
            out.append("")
        out += _docs(f.doc, "  ")
        nullable = not f.required and not f.has_default
        type_ = _dart_type(f.type)
        out.append(f"  final {type_ + '?' if nullable and not type_.endswith('?') else type_} {f.wire};")
    out.append("}")
    return out


def _dart_name(owner: str, wire: str) -> None:
    if wire in DART_KEYWORDS:
        raise SchemaError(f"{owner}.{wire}: a reserved word in Dart")


def _dart_enum(name: str, doc: str, values: List[str]) -> List[str]:
    out = _docs(doc, "")
    out.append(f"enum {name} {{")
    seen = {"unknown"}
    members = []
    for value in values:
        member = _lower_camel(value)
        if not member or member[0].isdigit() or member in seen or member in DART_KEYWORDS:
            raise SchemaError(f"{name}: {value!r} cannot be a Dart enum member")
        seen.add(member)
        members.append(f"  {member}({_dart_string(value)}),")
    out += members
    out += [
        "  unknown('unknown');",
        "",
        f"  const {name}(this.wire);",
        "",
        "  /// The value on the wire.",
        "  final String wire;",
        "",
        "  /// The case for a wire value; [unknown] for one this version does not know.",
        f"  static {name} parse(String raw) {{",
        f"    for (final {name} value in {name}.values) {{",
        "      if (value.wire == raw) return value;",
        "    }",
        f"    return {name}.unknown;",
        "  }",
        "}",
    ]
    return out


def dart_types(schema: Dict[str, Any]) -> str:
    model = Model(schema)
    blocks: List[str] = []

    def struct(s: Struct) -> None:
        blocks.append("\n".join(_dart_struct(model, s)) + "\n")
        for child in s.children:
            struct(child)

    for kind, item in model.items:
        if kind == "enum":
            blocks.append("\n".join(_dart_enum(*item)) + "\n")
        elif kind == "struct":
            struct(item)
        else:
            name, doc, target = item
            blocks.append("\n".join(_docs(doc, "") + [f"typedef {name} = {target};"]) + "\n")
    return "\n".join(blocks)


def dart_registry(schema: Dict[str, Any]) -> str:
    model = Model(schema)
    rows = "".join(f"  '{name}': (JsonMap json) => {name}.fromJson(json).toJson(),\n" for name in model.exported)
    return (
        "/// Parses a map as the type named by the key and writes it back.\n"
        "final Map<String, JsonMap Function(JsonMap)> roundtrips = <String, JsonMap Function(JsonMap)>{\n"
        f"{rows}"
        "};\n"
    )


DART_HEAD = """\
// GENERATED CODE - DO NOT MODIFY BY HAND.
// The agentconsole.v1 wire types, from protocol/agentconsole.v1.schema.json by tools/protocol/gen.py.
// Change the schema and run the generator. The app still reads the wire with models.dart.
// ignore_for_file: unused_element

/// A JSON object.
typedef JsonMap = Map<String, Object?>;

Never _fail(String want, Object? got) => throw FormatException('expected $want', got);

String _asString(Object? v) => v is String ? v : _fail('a string', v);

int _asInt(Object? v) => v is int ? v : (v is num && v == v.truncate() ? v.toInt() : _fail('an integer', v));

double _asDouble(Object? v) => v is num ? v.toDouble() : _fail('a number', v);

bool _asBool(Object? v) => v is bool ? v : _fail('a boolean', v);

Object? _asAny(Object? v) => v;

JsonMap _asMap(Object? v) => v is Map<Object?, Object?> ? v.cast<String, Object?>() : _fail('an object', v);

List<T> _asList<T>(Object? v, T Function(Object?) read) =>
    v is List<Object?> ? <T>[for (final Object? item in v) read(item)] : _fail('a list', v);

T _read<T>(JsonMap json, String key, T Function(Object?) read) {
  try {
    return read(json[key]);
  } on FormatException catch (error) {
    throw FormatException('"$key": ${error.message}', error.source);
  }
}

T _need<T>(JsonMap json, String key, T Function(Object?) read) {
  if (!json.containsKey(key)) throw FormatException('missing "$key"', json);
  return _read(json, key, read);
}

T? _maybe<T>(JsonMap json, String key, T Function(Object?) read) {
  if (json[key] == null) return null;
  return _read(json, key, read);
}

"""


def render_dart(schema: Dict[str, Any]) -> str:
    return DART_HEAD + dart_types(schema) + "\n" + dart_registry(schema)


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(description="Generate the Rust and Dart types of agentconsole.v1 from its schema.")
    parser.add_argument("--check", action="store_true", help="write nothing; exit 1 when a generated file differs")
    parser.add_argument("--schema", type=Path, default=SCHEMA)
    parser.add_argument("--rust", type=Path, default=RUST_OUT)
    parser.add_argument("--dart", type=Path, default=DART_OUT)
    args = parser.parse_args(argv)
    try:
        schema = json.loads(args.schema.read_text(encoding="utf-8"))
        outputs = [(args.rust, render_rust(schema)), (args.dart, render_dart(schema))]
    except (OSError, ValueError) as error:
        print(f"{RULE}: {args.schema}: {error}", file=sys.stderr)
        return 1
    if args.check:
        stale = [path for path, text in outputs if not path.is_file() or path.read_text(encoding="utf-8") != text]
        for path in stale:
            print(f"{RULE}: {path} is out of date with {args.schema.name}; run python3 tools/protocol/gen.py", file=sys.stderr)
        return 1 if stale else 0
    for path, text in outputs:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        print(f"wrote {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
