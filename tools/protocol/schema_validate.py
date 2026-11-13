from __future__ import annotations

from typing import Any, Dict, List

ANNOTATIONS = {"title", "description", "default", "$comment", "$schema", "$id", "version", "$defs"}
KEYWORDS = {"type", "const", "enum", "properties", "required", "additionalProperties", "items", "$ref", "oneOf", "anyOf"}


class SchemaError(ValueError):
    pass


def validate(instance: Any, schema: Dict[str, Any]) -> List[str]:
    return _check(instance, schema, schema, "$")


def _is_type(value: Any, name: str) -> bool:
    if name == "string":
        return isinstance(value, str)
    if name == "boolean":
        return isinstance(value, bool)
    if name == "integer":
        return (isinstance(value, int) and not isinstance(value, bool)) or (isinstance(value, float) and value.is_integer())
    if name == "number":
        return isinstance(value, (int, float)) and not isinstance(value, bool)
    if name == "object":
        return isinstance(value, dict)
    if name == "array":
        return isinstance(value, list)
    if name == "null":
        return value is None
    raise SchemaError(f"unknown type {name!r}")


def _same(a: Any, b: Any) -> bool:
    if isinstance(a, bool) != isinstance(b, bool):
        return False
    return a == b


def _resolve(root: Dict[str, Any], pointer: str) -> Dict[str, Any]:
    if not pointer.startswith("#/"):
        raise SchemaError(f"only local references are supported: {pointer!r}")
    node: Any = root
    for part in pointer[2:].split("/"):
        part = part.replace("~1", "/").replace("~0", "~")
        if not isinstance(node, dict) or part not in node:
            raise SchemaError(f"reference {pointer!r} points at nothing")
        node = node[part]
    return node


def _check(value: Any, schema: Dict[str, Any], root: Dict[str, Any], path: str) -> List[str]:
    if schema is True or schema == {}:
        return []
    if not isinstance(schema, dict):
        raise SchemaError(f"a schema must be an object at {path}")
    for key in schema:
        if key not in KEYWORDS and key not in ANNOTATIONS:
            raise SchemaError(f"unsupported keyword {key!r}")
    problems: List[str] = []

    if "$ref" in schema:
        problems += _check(value, _resolve(root, schema["$ref"]), root, path)

    if "type" in schema:
        names = schema["type"] if isinstance(schema["type"], list) else [schema["type"]]
        if not any(_is_type(value, n) for n in names):
            problems.append(f"{path}: expected {' or '.join(names)}, got {_describe(value)}")
            return problems

    if "const" in schema and not _same(value, schema["const"]):
        problems.append(f"{path}: expected {schema['const']!r}, got {value!r}")
    if "enum" in schema and not any(_same(value, option) for option in schema["enum"]):
        problems.append(f"{path}: {value!r} is not one of {schema['enum']}")

    if isinstance(value, dict):
        for name in schema.get("required", []):
            if name not in value:
                problems.append(f"{path}: missing required field {name!r}")
        properties = schema.get("properties", {})
        for name, item in value.items():
            if name in properties:
                problems += _check(item, properties[name], root, f"{path}.{name}")
            elif schema.get("additionalProperties") is False:
                problems.append(f"{path}: unexpected field {name!r}")
    if isinstance(value, list) and "items" in schema:
        for index, item in enumerate(value):
            problems += _check(item, schema["items"], root, f"{path}[{index}]")

    if "oneOf" in schema:
        matching = sum(1 for option in schema["oneOf"] if not _check(value, option, root, path))
        if matching != 1:
            problems.append(f"{path}: matches {matching} of the oneOf alternatives, expected exactly 1")
    if "anyOf" in schema and not any(not _check(value, option, root, path) for option in schema["anyOf"]):
        problems.append(f"{path}: matches none of the anyOf alternatives")
    return problems


def _describe(value: Any) -> str:
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "a boolean"
    return {str: "a string", int: "an integer", float: "a number", dict: "an object", list: "an array"}.get(type(value), type(value).__name__)
