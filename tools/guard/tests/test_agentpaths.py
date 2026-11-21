import json
import unittest

from guard import agentpaths
from guard.gate import agent_path_reasons
from tests.helpers import commit, head_of, make_repo

GOLDEN = str(agentpaths.GOLDEN)

WIRE = (
    "pub mod ops {\n"
    '    pub const A: &str = "a.one";\n'
    '    pub const B: &str = "a.two";\n'
    '    pub const C: &str = "a.three";\n'
    "}\n"
)


def golden(commands=(), planned=(), user_only=(), internal=()):
    return json.dumps({
        "commands": [
            {"command": c, "usage": f"notchling {c}", "summary": "s", "consent": "none", "exits": [0, 1],
             "ops": [{"op": op, "tier": "agent", "acts": False} for op in ops]}
            for c, ops in commands
        ],
        "planned": [{"op": op, "feature": "f", "phase": 2} for op in planned],
        "userOnly": [{"op": op, "feature": "f", "why": "w"} for op in user_only],
        "internal": [{"op": op, "why": "app_ui"} for op in internal],
    })


class OpTests(unittest.TestCase):
    def test_every_op_in_one_place_passes(self):
        text = golden(commands=[("a", ["a.one"])], planned=["a.two"], internal=["a.three"])
        self.assertEqual(agentpaths.check_ops(WIRE, text), [])

    def test_an_op_in_no_list_fails(self):
        text = golden(commands=[("a", ["a.one"])], planned=["a.two"])
        problems = agentpaths.check_ops(WIRE, text)
        self.assertEqual(len(problems), 1)
        self.assertIn("a.three", problems[0])
        self.assertTrue(problems[0].startswith("GUARD-AGENT-PATH:"))

    def test_an_op_in_two_lists_fails(self):
        text = golden(commands=[("a", ["a.one", "a.two"])], planned=["a.two"], internal=["a.three"])
        problems = agentpaths.check_ops(WIRE, text)
        self.assertEqual(len(problems), 1)
        self.assertIn("a.two", problems[0])
        self.assertIn("2 places", problems[0])

    def test_an_internal_op_needs_a_known_reason(self):
        text = golden(commands=[("a", ["a.one"])], planned=["a.two"], internal=["a.three"])
        data = json.loads(text)
        data["internal"][0]["why"] = "because"
        problems = agentpaths.check_ops(WIRE, json.dumps(data))
        self.assertEqual(len(problems), 1)
        self.assertIn("because", problems[0])

    def test_a_planned_op_needs_its_phase(self):
        data = json.loads(golden(commands=[("a", ["a.one"])], planned=["a.two"], internal=["a.three"]))
        del data["planned"][0]["phase"]
        problems = agentpaths.check_ops(WIRE, json.dumps(data))
        self.assertEqual(len(problems), 1)
        self.assertIn("no phase", problems[0])

    def test_a_wire_without_the_ops_module_fails_instead_of_passing_empty(self):
        text = golden(commands=[("a", ["a.one"])])
        for wire in ("// the ops moved\n", "pub mod topics {\n    pub const A: &str = \"a.one\";\n}\n"):
            problems = agentpaths.check_ops(wire, text)
            self.assertEqual(len(problems), 1, wire)
            self.assertIn("pub mod ops", problems[0])

    def test_a_constant_split_over_lines_is_still_an_op(self):
        wire = ("pub mod ops {\n    pub const A: &str =\n        \"a.one\";\n"
                "    pub const B: &str = \"a.two\";\n}\n")
        self.assertEqual(agentpaths.wire_ops(wire), ["a.one", "a.two"])
        problems = agentpaths.check_ops(wire, golden(commands=[("a", ["a.two"])]))
        self.assertEqual(len(problems), 1)
        self.assertIn("a.one", problems[0])

    def test_a_brace_inside_the_module_does_not_end_it(self):
        wire = ("pub mod ops {\n    pub const A: &str = \"a.one\";\n    pub mod inner {\n"
                "        pub const B: &str = \"a.two\";\n    }\n    pub const C: &str = \"a.three\";\n}\n"
                "pub const D: &str = \"not.an.op\";\n")
        self.assertEqual(agentpaths.wire_ops(wire), ["a.one", "a.two", "a.three"])

    def test_every_entry_needs_an_op(self):
        data = json.loads(golden(commands=[("a", ["a.one", "a.two"])], planned=["a.three"]))
        del data["planned"][0]["op"]
        del data["commands"][0]["ops"][1]["op"]
        problems = agentpaths.check_ops(WIRE, json.dumps(data))
        self.assertTrue(any("in planned has no op" in p or "entry in planned has no op" in p for p in problems))
        self.assertTrue(any("op without a name" in p for p in problems))

    def test_a_planned_phase_is_a_whole_number_from_2_to_9(self):
        for bad in (1, 10, "3", 2.5, True):
            data = json.loads(golden(commands=[("a", ["a.one"])], planned=["a.two"], internal=["a.three"]))
            data["planned"][0]["phase"] = bad
            problems = agentpaths.check_ops(WIRE, json.dumps(data))
            self.assertEqual(len(problems), 1, bad)
            self.assertIn("phase", problems[0])
        for good in (2, 9):
            data = json.loads(golden(commands=[("a", ["a.one"])], planned=["a.two"], internal=["a.three"]))
            data["planned"][0]["phase"] = good
            self.assertEqual(agentpaths.check_ops(WIRE, json.dumps(data)), [], good)

    def test_a_command_row_needs_a_real_usage_consent_exits_and_ops(self):
        base = json.loads(golden(commands=[("a", ["a.one", "a.two"])], internal=["a.three"]))
        for field, bad in (("usage", ""), ("usage", "a"), ("consent", "maybe"), ("exits", []), ("exits", ["0"])):
            data = json.loads(json.dumps(base))
            data["commands"][0][field] = bad
            problems = agentpaths.check_ops(WIRE, json.dumps(data))
            self.assertEqual(len(problems), 1, (field, bad))
        data = json.loads(json.dumps(base))
        data["commands"][0]["ops"][0]["tier"] = "free"
        data["commands"][0]["ops"][1]["acts"] = "yes"
        self.assertEqual(len(agentpaths.check_ops(WIRE, json.dumps(data))), 2)
        data = json.loads(json.dumps(base))
        data["commands"][0]["command"] = ""
        self.assertTrue(any("no name" in p for p in agentpaths.check_ops(WIRE, json.dumps(data))))

    def test_an_entry_that_is_not_an_object_fails_cleanly(self):
        data = json.loads(golden(commands=[("a", ["a.one"])]))
        data["planned"] = ["a.two"]
        self.assertIn("not an object", agentpaths.check_ops(WIRE, json.dumps(data))[0])

    def test_a_missing_or_broken_golden_fails(self):
        self.assertIn("missing", agentpaths.check_ops(WIRE, None)[0])
        self.assertIn("not JSON", agentpaths.check_ops(WIRE, "{ no")[0])
        self.assertIn("no commands", agentpaths.check_ops(WIRE, "{}")[0])


class GrowthTests(unittest.TestCase):
    def reasons(self, base, head):
        repo = make_repo({GOLDEN: base})
        base_sha = head_of(repo)
        head_sha = commit(repo, {GOLDEN: head})
        return agent_path_reasons(repo, base_sha, head_sha)

    def test_shrinking_is_fine(self):
        base = golden(planned=["a.two", "a.three"], internal=["a.one"])
        self.assertEqual(self.reasons(base, golden(planned=["a.two"], internal=[])), [])

    def test_moving_an_op_to_a_command_is_fine(self):
        base = golden(planned=["a.two"])
        self.assertEqual(self.reasons(base, golden(commands=[("a", ["a.two"])])), [])

    def test_a_planned_list_that_grew_is_flagged(self):
        reasons = self.reasons(golden(planned=["a.two"]), golden(planned=["a.two", "a.three"]))
        self.assertEqual(len(reasons), 1)
        self.assertTrue(reasons[0].startswith("GUARD-AGENT-PATH:"))
        self.assertIn("a.three", reasons[0])
        self.assertIn("planned", reasons[0])

    def test_an_internal_list_that_grew_is_flagged(self):
        reasons = self.reasons(golden(internal=["a.one"]), golden(internal=["a.one", "a.two"]))
        self.assertEqual(len(reasons), 1)
        self.assertIn("internal", reasons[0])
        self.assertIn("a.two", reasons[0])

    def test_a_new_user_only_op_is_flagged(self):
        reasons = self.reasons(golden(), golden(user_only=["a.one"]))
        self.assertEqual(len(reasons), 1)
        self.assertIn("userOnly", reasons[0])
        self.assertIn("a.one", reasons[0])

    def test_a_user_only_list_that_shrank_is_fine(self):
        self.assertEqual(self.reasons(golden(user_only=["a.one", "a.two"]), golden(user_only=["a.one"])), [])

    def test_an_unreadable_golden_at_the_head_is_flagged(self):
        reasons = self.reasons(golden(planned=["a.two"]), "{ no")
        self.assertEqual(len(reasons), 1)
        self.assertTrue(reasons[0].startswith("GUARD-AGENT-PATH:"))

    def test_a_repo_without_the_golden_is_fine(self):
        repo = make_repo({"x": "1\n"})
        base = head_of(repo)
        head = commit(repo, {"y": "2\n"})
        self.assertEqual(agent_path_reasons(repo, base, head), [])

    def test_a_new_golden_counts_as_growth(self):
        repo = make_repo({"x": "1\n"})
        base = head_of(repo)
        head = commit(repo, {GOLDEN: golden(planned=["a.two"])})
        reasons = agent_path_reasons(repo, base, head)
        self.assertEqual(len(reasons), 1)
        self.assertIn("grew by 1", reasons[0])


class SpecTests(unittest.TestCase):
    def test_an_old_design_needs_nothing(self):
        self.assertEqual(agentpaths.check_spec("2026-11-20-x-design.md", "# x\n"), [])

    def test_a_design_without_a_date_in_its_name_fails(self):
        for name in ("notes.md", "x-2026-12-01.md", "2026-12-x-design.md", "sub/design.md"):
            problems = agentpaths.check_spec(name, "# x\n")
            self.assertEqual(len(problems), 1, name)
            self.assertIn("date", problems[0])

    def test_a_dated_design_without_the_section_fails(self):
        problems = agentpaths.check_spec("2026-11-21-x-design.md", "# x\n\n## Goals\n")
        self.assertEqual(len(problems), 1)
        self.assertIn("## Agent path", problems[0])

    def test_a_command_and_an_op_pass(self):
        text = "# x\n\n## Agent path\n\n| Feature | Command | Op |\n| a | `notchling foo --json` | `foo.get` |\n\n## Risks\n"
        self.assertEqual(agentpaths.check_spec("2026-12-01-x-design.md", text), [])

    def test_a_section_with_only_words_fails(self):
        text = "# x\n\n## Agent path\n\nAgents will find a way.\n"
        self.assertEqual(len(agentpaths.check_spec("2026-12-01-x-design.md", text)), 1)

    def test_a_command_without_an_op_fails(self):
        text = "# x\n\n## Agent path\n\nRun `notchling foo`.\n"
        self.assertEqual(len(agentpaths.check_spec("2026-12-01-x-design.md", text)), 1)

    def test_a_file_name_or_a_word_with_a_dot_is_not_an_op(self):
        for prose in ("See `notchling help` and `settings.json`.", "Run `notchling foo`, e.g. `main.rs`.",
                      "Use `notchling foo` with `e.g`.", "`notchling foo` reads `Cargo.toml`."):
            text = f"# x\n\n## Agent path\n\n{prose}\n"
            self.assertEqual(len(agentpaths.check_spec("2026-12-01-x-design.md", text)), 1, prose)

    def test_the_command_and_the_op_share_a_line(self):
        text = "# x\n\n## Agent path\n\nRun `notchling foo`.\n\nIt calls `foo.get`.\n"
        self.assertEqual(len(agentpaths.check_spec("2026-12-01-x-design.md", text)), 1)
        bullet = "# x\n\n## Agent path\n\n- `notchling foo --json` calls `foo.get`.\n"
        self.assertEqual(agentpaths.check_spec("2026-12-01-x-design.md", bullet), [])

    def test_no_agent_path_must_start_a_line(self):
        quoted = "# x\n\n## Agent path\n\nWe will not write \"No agent path: app_ui\" here.\n"
        self.assertEqual(len(agentpaths.check_spec("2026-12-01-x-design.md", quoted)), 1)
        bullet = "# x\n\n## Agent path\n\n- No agent path: hook, the hook's own call.\n"
        self.assertEqual(agentpaths.check_spec("2026-12-01-x-design.md", bullet), [])

    def test_naming_one_outside_the_section_does_not_count(self):
        text = "# x\n\nRun `notchling foo` for `foo.get`.\n\n## Agent path\n\nLater.\n"
        self.assertEqual(len(agentpaths.check_spec("2026-12-01-x-design.md", text)), 1)

    def test_no_agent_path_needs_a_reason_from_the_internal_list(self):
        ok = "# x\n\n## Agent path\n\nNo agent path: app_ui, the app's own view.\n"
        self.assertEqual(agentpaths.check_spec("2026-12-01-x-design.md", ok), [])
        bad = "# x\n\n## Agent path\n\nNo agent path: we forgot.\n"
        self.assertEqual(len(agentpaths.check_spec("2026-12-01-x-design.md", bad)), 1)

    def test_check_reads_the_wire_the_golden_and_the_specs(self):
        repo = make_repo({
            "crates/local-api/src/wire.rs": WIRE,
            GOLDEN: golden(commands=[("a", ["a.one", "a.two"])], planned=["a.three"]),
            "docs/superpowers/specs/2026-12-01-x-design.md": "# x\n",
        })
        problems = agentpaths.check(repo)
        self.assertEqual(len(problems), 1)
        self.assertIn("2026-12-01-x-design.md", problems[0])


    def test_check_walks_subfolders_and_flags_undated_files(self):
        repo = make_repo({
            "crates/local-api/src/wire.rs": WIRE,
            GOLDEN: golden(commands=[("a", ["a.one", "a.two"])], planned=["a.three"]),
            "docs/superpowers/specs/sub/2026-12-01-deep-design.md": "# x\n",
            "docs/superpowers/specs/notes.md": "# x\n",
            "docs/superpowers/specs/2026-12-02-ok-design.md":
                "# x\n\n## Agent path\n\n| a | `notchling a` | `foo.get` |\n",
        })
        problems = agentpaths.check(repo)
        self.assertEqual(len(problems), 2, problems)
        self.assertTrue(any("sub/2026-12-01-deep-design.md" in p for p in problems))
        self.assertTrue(any("notes.md" in p and "date" in p for p in problems))

    def test_check_fails_when_the_wire_lost_its_ops_module(self):
        repo = make_repo({
            "crates/local-api/src/wire.rs": "pub mod renamed {\n}\n",
            GOLDEN: golden(commands=[("a", ["a.one"])]),
        })
        problems = agentpaths.check(repo)
        self.assertEqual(len(problems), 1)
        self.assertIn("pub mod ops", problems[0])


if __name__ == "__main__":
    unittest.main()
