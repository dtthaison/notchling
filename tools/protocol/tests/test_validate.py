from __future__ import annotations

import unittest

from schema_validate import SchemaError, validate


def errors(instance, schema, defs=None):
    root = dict(schema)
    if defs:
        root["$defs"] = defs
    return validate(instance, root)


class TypeTests(unittest.TestCase):
    def test_each_type_accepts_its_own_values(self):
        for kind, value in [("string", "x"), ("integer", 3), ("number", 3), ("number", 2.5), ("boolean", False),
                            ("object", {}), ("array", []), ("null", None), ("integer", 4.0)]:
            with self.subTest(kind=kind, value=value):
                self.assertEqual(errors(value, {"type": kind}), [])

    def test_each_type_rejects_the_others(self):
        for kind, value in [("string", 1), ("integer", "1"), ("integer", 2.5), ("number", "1"), ("boolean", 1),
                            ("object", []), ("array", {}), ("null", 0)]:
            with self.subTest(kind=kind, value=value):
                self.assertEqual(len(errors(value, {"type": kind})), 1)

    def test_a_bool_is_not_a_number(self):
        self.assertEqual(len(errors(True, {"type": "integer"})), 1)
        self.assertEqual(len(errors(True, {"type": "number"})), 1)

    def test_type_list(self):
        self.assertEqual(errors("a", {"type": ["string", "null"]}), [])
        self.assertEqual(len(errors(1, {"type": ["string", "null"]})), 1)


class ObjectTests(unittest.TestCase):
    SCHEMA = {"type": "object", "properties": {"a": {"type": "string"}, "b": {"type": "integer"}}, "required": ["a"]}

    def test_valid(self):
        self.assertEqual(errors({"a": "x", "b": 1}, self.SCHEMA), [])

    def test_missing_required_names_the_field(self):
        found = errors({"b": 1}, self.SCHEMA)
        self.assertEqual(len(found), 1)
        self.assertIn("a", found[0])

    def test_wrong_property_type_names_the_path(self):
        found = errors({"a": "x", "b": "no"}, self.SCHEMA)
        self.assertEqual(len(found), 1)
        self.assertIn("$.b", found[0])

    def test_extra_fields_are_allowed_by_default_and_when_true(self):
        self.assertEqual(errors({"a": "x", "extra": 1}, self.SCHEMA), [])
        self.assertEqual(errors({"a": "x", "extra": 1}, dict(self.SCHEMA, additionalProperties=True)), [])

    def test_additional_properties_false_rejects(self):
        self.assertEqual(len(errors({"a": "x", "extra": 1}, dict(self.SCHEMA, additionalProperties=False))), 1)


class ValueTests(unittest.TestCase):
    def test_enum(self):
        self.assertEqual(errors("a", {"enum": ["a", "b"]}), [])
        self.assertEqual(len(errors("c", {"type": "string", "enum": ["a", "b"]})), 1)

    def test_const(self):
        self.assertEqual(errors("hello", {"const": "hello"}), [])
        self.assertEqual(len(errors("hi", {"const": "hello"})), 1)
        self.assertEqual(errors(True, {"const": True}), [])
        self.assertEqual(len(errors(1, {"const": True})), 1, "1 is not true")

    def test_items(self):
        self.assertEqual(errors([1, 2], {"type": "array", "items": {"type": "integer"}}), [])
        found = errors([1, "x"], {"type": "array", "items": {"type": "integer"}})
        self.assertEqual(len(found), 1)
        self.assertIn("$[1]", found[0])


class CombinatorTests(unittest.TestCase):
    DEFS = {"Num": {"type": "integer"}, "Str": {"type": "string"}}

    def test_ref(self):
        self.assertEqual(errors(1, {"$ref": "#/$defs/Num"}, self.DEFS), [])
        self.assertEqual(len(errors("x", {"$ref": "#/$defs/Num"}, self.DEFS)), 1)

    def test_unknown_ref_is_a_schema_error(self):
        with self.assertRaises(SchemaError):
            errors(1, {"$ref": "#/$defs/Nope"}, self.DEFS)

    def test_one_of_needs_exactly_one_match(self):
        schema = {"oneOf": [{"$ref": "#/$defs/Num"}, {"$ref": "#/$defs/Str"}]}
        self.assertEqual(errors(1, schema, self.DEFS), [])
        self.assertEqual(errors("x", schema, self.DEFS), [])
        self.assertEqual(len(errors(True, schema, self.DEFS)), 1, "matches none")
        both = {"oneOf": [{"type": "number"}, {"type": "integer"}]}
        self.assertEqual(len(errors(1, both)), 1, "matches two")

    def test_any_of_needs_one_match(self):
        schema = {"anyOf": [{"type": "number"}, {"type": "integer"}]}
        self.assertEqual(errors(1, schema), [])
        self.assertEqual(len(errors("x", schema)), 1)

    def test_ref_with_sibling_keywords(self):
        self.assertEqual(errors(1, {"description": "d", "$ref": "#/$defs/Num"}, self.DEFS), [])


class UnsupportedTests(unittest.TestCase):
    def test_a_keyword_the_validator_does_not_know_is_an_error_not_a_pass(self):
        with self.assertRaises(SchemaError):
            errors("x", {"pattern": "^y$"})

    def test_annotations_are_ignored(self):
        schema = {"title": "t", "description": "d", "default": 1, "$comment": "c", "type": "integer"}
        self.assertEqual(errors(1, schema), [])


if __name__ == "__main__":
    unittest.main()
