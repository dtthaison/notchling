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
        "commands": [{"command": c, "ops": [{"op": op} for op in ops]} for c, ops in commands],
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

    def test_a_new_user_only_op_is_not_flagged(self):
        self.assertEqual(self.reasons(golden(), golden(user_only=["a.one"])), [])

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
        self.assertEqual(agentpaths.check_spec("notes.md", "# x\n"), [])

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


if __name__ == "__main__":
    unittest.main()
