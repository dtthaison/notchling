from __future__ import annotations

import copy
import json
import re
import unittest
from pathlib import Path

from schema_validate import validate

ROOT = Path(__file__).resolve().parents[3]
SCHEMA_FILE = ROOT / "protocol/agentconsole.v1.schema.json"
FIXTURES = ROOT / "protocol/fixtures"
ROUTER = ROOT / "legacy/macos/Sources/ConsoleCore/Bridge/BridgeRouter.swift"
HUB = ROOT / "legacy/macos/Sources/ConsoleCore/Bridge/BridgeHub.swift"

SCHEMA = json.loads(SCHEMA_FILE.read_text(encoding="utf-8"))
DEFS = SCHEMA["$defs"]


def fixture_files():
    return sorted(FIXTURES.glob("*.json"))


def def_name(path: Path) -> str:
    return path.name.split(".")[0]


def at_def(name: str) -> dict:
    return {"$ref": "#/$defs/" + name, "$defs": DEFS}


class SchemaShapeTests(unittest.TestCase):
    def test_version(self):
        self.assertEqual(SCHEMA["version"], "agentconsole.v1")
        self.assertEqual(SCHEMA["$schema"], "https://json-schema.org/draft/2020-12/schema")

    def test_every_object_allows_extra_fields(self):
        for name, definition in DEFS.items():
            if definition.get("type") == "object":
                with self.subTest(name=name):
                    self.assertIs(definition.get("additionalProperties"), True)

    def test_the_root_rejects_an_empty_object(self):
        self.assertNotEqual(validate({}, SCHEMA), [])


class FixtureTests(unittest.TestCase):
    def test_there_are_fixtures(self):
        self.assertGreater(len(fixture_files()), 60)

    def test_every_fixture_names_a_definition_and_validates(self):
        for path in fixture_files():
            with self.subTest(fixture=path.name):
                name = def_name(path)
                self.assertIn(name, DEFS, f"{path.name} does not start with a definition name")
                self.assertEqual(validate(json.loads(path.read_text(encoding="utf-8")), at_def(name)), [])

    def test_every_request_and_event_fixture_also_matches_the_union_it_belongs_to(self):
        for path in fixture_files():
            name = def_name(path)
            union = "Request" if name.endswith("Request") else "Event" if name.endswith("Event") else None
            if union is None:
                continue
            with self.subTest(fixture=path.name):
                self.assertEqual(validate(json.loads(path.read_text(encoding="utf-8")), at_def(union)), [])

    def test_every_error_code_has_a_fixture(self):
        codes = set(DEFS["ErrorCode"]["enum"])
        seen = {json.loads(p.read_text(encoding="utf-8"))["error"]["code"] for p in fixture_files() if def_name(p) == "ErrorResponse"}
        self.assertEqual(seen, codes)

    def test_every_definition_with_a_wire_shape_has_a_fixture(self):
        have = {def_name(p) for p in fixture_files()}
        wire = [n for n in DEFS if n.endswith(("Request", "Response", "Event")) and n not in ("Request", "Event")]
        self.assertEqual([n for n in wire if n not in have], [])


class BadExampleTests(unittest.TestCase):

    def load(self, name):
        return json.loads((FIXTURES / (name + ".json")).read_text(encoding="utf-8"))

    def test_dropping_a_required_field_fails(self):
        for name, key in [("SessionsListResponse", "data"), ("ChatGetRequest", "sessionId"), ("HelloRequest", "token"),
                          ("ErrorResponse.busy", "error"), ("OpenEvent", "event"), ("PingResponse", "ok")]:
            with self.subTest(fixture=name, dropped=key):
                value = self.load(name)
                del value[key]
                self.assertNotEqual(validate(value, at_def(name.split(".")[0])), [])

    def test_a_nested_required_field_fails(self):
        value = self.load("SessionsListResponse")
        del value["data"]["sessions"][0]["state"]
        self.assertNotEqual(validate(value, at_def("SessionsListResponse")), [])

    def test_a_wrong_type_fails(self):
        value = self.load("ChatGetResponse")
        value["data"]["messages"][0]["time"] = "yesterday"
        self.assertNotEqual(validate(value, at_def("ChatGetResponse")), [])

    def test_an_unknown_enum_value_fails(self):
        value = self.load("SessionsListResponse")
        value["data"]["sessions"][0]["state"] = "sleeping"
        self.assertNotEqual(validate(value, at_def("SessionsListResponse")), [])

    def test_a_wrong_op_matches_no_request(self):
        value = self.load("PingRequest")
        value["op"] = "nope"
        self.assertNotEqual(validate(value, at_def("Request")), [])

    def test_an_extra_field_is_fine(self):
        value = copy.deepcopy(self.load("SessionsListResponse"))
        value["data"]["sessions"][0]["somethingNew"] = {"a": 1}
        self.assertEqual(validate(value, at_def("SessionsListResponse")), [])


class RouterCoverageTests(unittest.TestCase):
    def test_every_op_of_the_router_is_in_the_schema_with_a_fixture(self):
        text = ROUTER.read_text(encoding="utf-8")
        ops = set(re.findall(r'^\s*case "([a-z][A-Za-z.]*)":', text, re.M))
        ops |= {"hello", "pair"}
        self.assertGreater(len(ops), 18)
        in_schema = set()
        for ref in DEFS["Request"]["oneOf"]:
            in_schema.add(DEFS[ref["$ref"].split("/")[-1]]["properties"]["op"]["const"])
        self.assertEqual(sorted(ops - in_schema), [], "ops the router handles but the schema lacks")
        self.assertEqual(sorted(in_schema - ops), [], "ops the schema has but the router lacks")
        fixture_ops = {json.loads(p.read_text(encoding="utf-8"))["op"] for p in fixture_files() if def_name(p).endswith("Request")}
        self.assertEqual(sorted(in_schema - fixture_ops), [])

    def test_every_event_the_hub_sends_is_in_the_schema(self):
        text = HUB.read_text(encoding="utf-8") + ROUTER.read_text(encoding="utf-8")
        events = set(re.findall(r'BridgeWire\.event\("([a-z]+)"', text))
        in_schema = {DEFS[r["$ref"].split("/")[-1]]["properties"]["event"]["const"] for r in DEFS["Event"]["oneOf"]}
        self.assertGreaterEqual(len(events), 4)
        self.assertEqual(sorted(events - in_schema), [])

    def test_every_error_code_of_the_router_is_in_the_schema(self):
        text = (ROOT / "legacy/macos/Sources/ConsoleCore/Bridge/BridgeProtocol.swift").read_text(encoding="utf-8")
        block = re.search(r"enum BridgeErrorCode[^{]*\{(.*?)\n\}", text, re.S).group(1)
        codes = set()
        for line in block.splitlines():
            line = line.strip()
            if not line.startswith("case "):
                continue
            for part in line[5:].split(","):
                match = re.match(r'`?(\w+)`?(?: = "(\w+)")?$', part.strip())
                codes.add(match.group(2) or match.group(1))
        self.assertEqual(codes, set(DEFS["ErrorCode"]["enum"]))


if __name__ == "__main__":
    unittest.main()
