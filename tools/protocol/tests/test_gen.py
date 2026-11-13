from __future__ import annotations

import copy
import io
import json
import shutil
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

import gen

ROOT = Path(__file__).resolve().parents[3]

TINY = {
    "$defs": {
        "Color": {"description": "A color.", "type": "string", "enum": ["red", "dark_blue"]},
        "Dot": {
            "description": "A dot.",
            "type": "object",
            "additionalProperties": True,
            "properties": {
                "id": {"type": "string"},
                "colorName": {"$ref": "#/$defs/Color", "description": "Its color."},
                "size": {"type": "integer", "default": 1},
                "note": {"type": "string"},
                "tags": {"type": "array", "items": {"type": "string"}},
                "webURL": {"type": "string"},
                "kind": {"type": "string", "const": "dot"},
            },
            "required": ["id", "colorName", "kind"],
        },
        "Pack": {
            "type": "object",
            "properties": {
                "dots": {"type": "array", "items": {"$ref": "#/$defs/Dot"}, "default": []},
                "main": {"$ref": "#/$defs/Dot"},
                "meta": {"type": "object", "properties": {"n": {"type": "number"}}, "required": ["n"]},
                "any": {"oneOf": [{"type": "string"}, {"type": "integer"}]},
            },
        },
        "DotAlias": {"description": "Same.", "$ref": "#/$defs/Dot"},
        "Either": {"oneOf": [{"$ref": "#/$defs/Dot"}, {"$ref": "#/$defs/Pack"}]},
    }
}

EXPECTED_RUST = """\
/// A color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Color {
    #[serde(rename = "red")]
    Red,
    #[serde(rename = "dark_blue")]
    DarkBlue,
    #[serde(other)]
    Unknown,
}

/// A dot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dot {
    pub id: String,
    /// Its color.
    pub color_name: Color,
    #[serde(default = "default_dot_size")]
    pub size: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "webURL")]
    pub web_url: Option<String>,
    pub kind: String,
}

fn default_dot_size() -> i64 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pack {
    #[serde(default)]
    pub dots: Vec<Dot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main: Option<Dot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<PackMeta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub any: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackMeta {
    pub n: f64,
}

/// Same.
pub type DotAlias = Dot;
"""

EXPECTED_RUST_REGISTRY = """\
/// Parses `value` as the type named `name` and writes it back. `None` when the schema has no such type.
pub fn roundtrip(name: &str, value: &Value) -> Option<Result<Value, serde_json::Error>> {
    Some(match name {
        "Dot" => round::<Dot>(value),
        "Pack" => round::<Pack>(value),
        "DotAlias" => round::<DotAlias>(value),
        _ => return None,
    })
}
"""

EXPECTED_DART = """\
/// A color.
enum Color {
  red('red'),
  darkBlue('dark_blue'),
  unknown('unknown');

  const Color(this.wire);

  /// The value on the wire.
  final String wire;

  /// The case for a wire value; [unknown] for one this version does not know.
  static Color parse(String raw) {
    for (final Color value in Color.values) {
      if (value.wire == raw) return value;
    }
    return Color.unknown;
  }
}

/// A dot.
class Dot {
  const Dot({
    required this.id,
    required this.colorName,
    this.size = 1,
    this.note,
    this.tags,
    this.webURL,
    this.kind = 'dot',
  });

  factory Dot.fromJson(JsonMap json) => Dot(
    id: _need(json, 'id', _asString),
    colorName: _need(json, 'colorName', (Object? v) => Color.parse(_asString(v))),
    size: _maybe(json, 'size', _asInt) ?? 1,
    note: _maybe(json, 'note', _asString),
    tags: _maybe(json, 'tags', (Object? v) => _asList<String>(v, _asString)),
    webURL: _maybe(json, 'webURL', _asString),
    kind: _need(json, 'kind', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    'id': id,
    'colorName': colorName.wire,
    'size': size,
    if (note != null) 'note': note,
    if (tags != null) 'tags': tags,
    if (webURL != null) 'webURL': webURL,
    'kind': kind,
  };

  final String id;

  /// Its color.
  final Color colorName;
  final int size;
  final String? note;
  final List<String>? tags;
  final String? webURL;
  final String kind;
}

class Pack {
  const Pack({
    this.dots = const <Dot>[],
    this.main,
    this.meta,
    this.any,
  });

  factory Pack.fromJson(JsonMap json) => Pack(
    dots: _maybe(json, 'dots', (Object? v) => _asList<Dot>(v, (Object? e) => Dot.fromJson(_asMap(e)))) ?? const <Dot>[],
    main: _maybe(json, 'main', (Object? v) => Dot.fromJson(_asMap(v))),
    meta: _maybe(json, 'meta', (Object? v) => PackMeta.fromJson(_asMap(v))),
    any: json['any'],
  );

  JsonMap toJson() => <String, Object?>{
    'dots': <Object?>[for (final Dot e in dots) e.toJson()],
    if (main != null) 'main': main!.toJson(),
    if (meta != null) 'meta': meta!.toJson(),
    if (any != null) 'any': any,
  };

  final List<Dot> dots;
  final Dot? main;
  final PackMeta? meta;
  final Object? any;
}

class PackMeta {
  const PackMeta({
    required this.n,
  });

  factory PackMeta.fromJson(JsonMap json) => PackMeta(
    n: _need(json, 'n', _asDouble),
  );

  JsonMap toJson() => <String, Object?>{
    'n': n,
  };

  final double n;
}

/// Same.
typedef DotAlias = Dot;
"""

EXPECTED_DART_REGISTRY = """\
/// Parses a map as the type named by the key and writes it back.
final Map<String, JsonMap Function(JsonMap)> roundtrips = <String, JsonMap Function(JsonMap)>{
  'Dot': (JsonMap json) => Dot.fromJson(json).toJson(),
  'Pack': (JsonMap json) => Pack.fromJson(json).toJson(),
  'DotAlias': (JsonMap json) => DotAlias.fromJson(json).toJson(),
};
"""


class TinySchemaTests(unittest.TestCase):
    def test_rust_types(self):
        self.assertEqual(gen.rust_types(TINY), EXPECTED_RUST)

    def test_rust_registry(self):
        self.assertEqual(gen.rust_registry(TINY), EXPECTED_RUST_REGISTRY)

    def test_dart_types(self):
        self.assertEqual(gen.dart_types(TINY), EXPECTED_DART)

    def test_dart_registry(self):
        self.assertEqual(gen.dart_registry(TINY), EXPECTED_DART_REGISTRY)

    def test_a_union_is_not_a_type(self):
        self.assertNotIn("Either", gen.rust_types(TINY))
        self.assertNotIn("Either", gen.dart_types(TINY))

    def test_full_files_wrap_the_pieces(self):
        rust = gen.render_rust(TINY)
        self.assertTrue(rust.startswith("//! "), "a module doc comment first")
        self.assertIn("@generated", rust)
        self.assertIn("use serde::", rust)
        self.assertIn(EXPECTED_RUST, rust)
        self.assertIn(EXPECTED_RUST_REGISTRY, rust)
        dart = gen.render_dart(TINY)
        self.assertTrue(dart.startswith("// "))
        self.assertIn("GENERATED", dart)
        self.assertIn(EXPECTED_DART, dart)
        self.assertIn(EXPECTED_DART_REGISTRY, dart)
        for text in (rust, dart):
            self.assertTrue(text.endswith("\n") and not text.endswith("\n\n"))


class RejectsWhatItCannotWriteTests(unittest.TestCase):
    def test_unknown_keyword_in_a_property(self):
        bad = copy.deepcopy(TINY)
        bad["$defs"]["Dot"]["properties"]["id"]["pattern"] = "^a$"
        with self.assertRaises(gen.SchemaError):
            gen.rust_types(bad)

    def test_ref_to_a_missing_definition(self):
        bad = copy.deepcopy(TINY)
        bad["$defs"]["Dot"]["properties"]["colorName"] = {"$ref": "#/$defs/Nope"}
        with self.assertRaises(gen.SchemaError):
            gen.dart_types(bad)

    def test_required_name_that_is_not_a_property(self):
        bad = copy.deepcopy(TINY)
        bad["$defs"]["Dot"]["required"].append("ghost")
        with self.assertRaises(gen.SchemaError):
            gen.rust_types(bad)


class DeterminismTests(unittest.TestCase):
    def test_same_schema_same_text(self):
        schema = json.loads((ROOT / "protocol/agentconsole.v1.schema.json").read_text(encoding="utf-8"))
        self.assertEqual(gen.render_rust(schema), gen.render_rust(copy.deepcopy(schema)))
        self.assertEqual(gen.render_dart(schema), gen.render_dart(copy.deepcopy(schema)))

    def test_every_object_definition_of_the_real_schema_is_a_type(self):
        schema = json.loads((ROOT / "protocol/agentconsole.v1.schema.json").read_text(encoding="utf-8"))
        rust, dart = gen.render_rust(schema), gen.render_dart(schema)
        for name, definition in schema["$defs"].items():
            if definition.get("type") == "object":
                with self.subTest(name=name):
                    self.assertIn(f"pub struct {name} ", rust)
                    self.assertIn(f"class {name} ", dart)


class CheckModeTests(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp(prefix="gen-test-"))
        self.addCleanup(shutil.rmtree, self.dir, True)
        self.schema = self.dir / "schema.json"
        self.rust = self.dir / "out/generated.rs"
        self.dart = self.dir / "out/generated.dart"
        self.schema.write_text(json.dumps(TINY), encoding="utf-8")

    def run_gen(self, *extra):
        out, err = io.StringIO(), io.StringIO()
        with redirect_stdout(out), redirect_stderr(err):
            code = gen.main(["--schema", str(self.schema), "--rust", str(self.rust), "--dart", str(self.dart), *extra])
        return code, out.getvalue() + err.getvalue()

    def test_check_fails_when_the_files_are_missing(self):
        code, text = self.run_gen("--check")
        self.assertEqual(code, 1)
        self.assertIn("generated.rs", text)

    def test_generate_then_check_passes(self):
        self.assertEqual(self.run_gen()[0], 0)
        self.assertEqual(self.rust.read_text(encoding="utf-8"), gen.render_rust(TINY))
        self.assertEqual(self.dart.read_text(encoding="utf-8"), gen.render_dart(TINY))
        self.assertEqual(self.run_gen("--check")[0], 0)

    def test_check_does_not_write(self):
        self.run_gen("--check")
        self.assertFalse(self.rust.exists())

    def test_check_fails_when_a_generated_file_was_edited(self):
        self.run_gen()
        self.dart.write_text(self.dart.read_text(encoding="utf-8") + "// edited\n", encoding="utf-8")
        code, text = self.run_gen("--check")
        self.assertEqual(code, 1)
        self.assertIn("generated.dart", text)
        self.assertNotIn("generated.rs", text)

    def test_check_fails_when_the_schema_changed(self):
        self.run_gen()
        changed = copy.deepcopy(TINY)
        changed["$defs"]["Dot"]["properties"]["extra"] = {"type": "string"}
        self.schema.write_text(json.dumps(changed), encoding="utf-8")
        self.assertEqual(self.run_gen("--check")[0], 1)

    def test_a_schema_the_generator_cannot_read_fails_with_a_message(self):
        bad = copy.deepcopy(TINY)
        bad["$defs"]["Dot"]["properties"]["id"]["pattern"] = "x"
        self.schema.write_text(json.dumps(bad), encoding="utf-8")
        code, text = self.run_gen()
        self.assertEqual(code, 1)
        self.assertIn("pattern", text)


class CommittedFilesTests(unittest.TestCase):
    def test_the_committed_generated_files_match_the_schema(self):
        out = io.StringIO()
        with redirect_stdout(out), redirect_stderr(out):
            code = gen.main(["--check"])
        self.assertEqual(code, 0, out.getvalue())


if __name__ == "__main__":
    unittest.main()
