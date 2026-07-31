#!/usr/bin/env python3
"""Tests for specs/006-public-scope-and-identity FR-002/FR-003/FR-004 enforcement
in scripts/ci/capability_validation.py (registry issue #22).

Run with: python3 -m unittest scripts/ci/tests/test_capability_validation.py
"""

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).resolve().parents[1] / "capability_validation.py"
spec = importlib.util.spec_from_file_location("capability_validation", MODULE_PATH)
capability_validation = importlib.util.module_from_spec(spec)
sys.modules["capability_validation"] = capability_validation
spec.loader.exec_module(capability_validation)


def valid_contract():
    return {
        "id": "example-capability",
        "namespace": "core",
        "owner": {"team": "platform"},
        "version": "1.0.0",
    }


def write_contract(tmp_dir: str, contract: dict, namespace="core", cap_id="example-capability", version="1.0.0") -> Path:
    path = Path(tmp_dir) / "capabilities" / namespace / cap_id / version / "contract.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(contract))
    return path


class CapabilityValidationSpec006Tests(unittest.TestCase):
    def test_valid_seed_shaped_contract_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_contract(tmp, valid_contract())
            errors: list = []
            capability_validation.validate_contract(path, errors)
            self.assertEqual(errors, [])

    def test_owner_missing_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            contract = valid_contract()
            del contract["owner"]
            path = write_contract(tmp, contract)
            errors: list = []
            capability_validation.validate_contract(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("contract.missing_required_field", codes)

    def test_owner_non_object_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            contract = valid_contract()
            contract["owner"] = "core"
            path = write_contract(tmp, contract)
            errors: list = []
            capability_validation.validate_contract(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("contract.invalid_owner", codes)

    def test_owner_missing_team_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            contract = valid_contract()
            contract["owner"] = {"contact": "team@example.com"}
            path = write_contract(tmp, contract)
            errors: list = []
            capability_validation.validate_contract(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("contract.invalid_owner", codes)

    def test_owner_empty_team_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            contract = valid_contract()
            contract["owner"] = {"team": "   "}
            path = write_contract(tmp, contract)
            errors: list = []
            capability_validation.validate_contract(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("contract.invalid_owner", codes)

    def test_top_level_scope_field_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            contract = valid_contract()
            contract["scope"] = "public"
            path = write_contract(tmp, contract)
            errors: list = []
            capability_validation.validate_contract(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("contract.forbidden_scope_field", codes)

    def test_empty_namespace_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            contract = valid_contract()
            contract["namespace"] = ""
            path = write_contract(tmp, contract, namespace="core")
            errors: list = []
            capability_validation.validate_contract(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("contract.invalid_namespace", codes)

    def test_non_string_namespace_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            contract = valid_contract()
            contract["namespace"] = 123
            path = write_contract(tmp, contract, namespace="core")
            errors: list = []
            capability_validation.validate_contract(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("contract.invalid_namespace", codes)

    def test_mismatched_namespace_still_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            contract = valid_contract()
            contract["namespace"] = "other"
            path = write_contract(tmp, contract, namespace="core")
            errors: list = []
            capability_validation.validate_contract(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("contract.namespace_mismatch", codes)
            self.assertNotIn("contract.invalid_namespace", codes)


def valid_workflow():
    return {
        "kind": "workflow_definition",
        "id": "example.workflow",
        "namespace": "example",
        "owner": {"team": "platform"},
        "version": "1.0.0",
        "nodes": [],
        "edges": [],
        "start_node": "n1",
        "terminal_nodes": ["n1"],
    }


def write_workflow(tmp_dir: str, workflow: dict, namespace="example", workflow_id="example.workflow", version="1.0.0") -> Path:
    path = Path(tmp_dir) / "workflows" / namespace / workflow_id / version / "workflow.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(workflow))
    return path


class WorkflowValidationFR013Tests(unittest.TestCase):
    """registry#124: workflows governed the same way capabilities are (spec 001 FR-013)."""

    def test_valid_workflow_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_workflow(tmp, valid_workflow())
            errors: list = []
            capability_validation.validate_workflow(path, errors)
            self.assertEqual(errors, [])

    def test_missing_required_field_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            workflow = valid_workflow()
            del workflow["start_node"]
            path = write_workflow(tmp, workflow)
            errors: list = []
            capability_validation.validate_workflow(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("workflow.missing_required_field", codes)

    def test_namespace_mismatch_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            workflow = valid_workflow()
            workflow["namespace"] = "other"
            path = write_workflow(tmp, workflow, namespace="example")
            errors: list = []
            capability_validation.validate_workflow(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("workflow.namespace_mismatch", codes)

    def test_id_mismatch_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            workflow = valid_workflow()
            workflow["id"] = "different.workflow"
            path = write_workflow(tmp, workflow, workflow_id="example.workflow")
            errors: list = []
            capability_validation.validate_workflow(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("workflow.id_mismatch", codes)

    def test_invalid_semver_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            workflow = valid_workflow()
            workflow["version"] = "not-a-version"
            path = write_workflow(tmp, workflow, version="not-a-version")
            errors: list = []
            capability_validation.validate_workflow(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("workflow.invalid_semver", codes)

    def test_invalid_json_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "workflows" / "example" / "example.workflow" / "1.0.0" / "workflow.json"
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("{ not valid json")
            errors: list = []
            capability_validation.validate_workflow(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("workflow.invalid_json", codes)


class ScenarioUserStoryFormatTests(unittest.TestCase):
    """registry#140, decision-log entry 46: use_cases[].scenario must be a
    full user story. is_user_story_scenario is deliberately permissive
    (substring presence + order), not a rigid regex -- see its docstring."""

    def test_valid_user_story_passes(self):
        self.assertTrue(
            capability_validation.is_user_story_scenario(
                "As a developer, I want to validate an email address, so that I can reject malformed input early."
            )
        )

    def test_plain_declarative_sentence_fails(self):
        self.assertFalse(capability_validation.is_user_story_scenario("A well-formed address is accepted."))

    def test_missing_so_that_clause_fails(self):
        self.assertFalse(capability_validation.is_user_story_scenario("As a developer, I want to validate an email."))

    def test_wrong_clause_order_fails(self):
        # "so that" appearing before "i want" is not a valid story, even
        # though all three substrings are technically present somewhere.
        self.assertFalse(
            capability_validation.is_user_story_scenario("So that signups work, as a developer I want validation.")
        )

    def test_non_string_scenario_fails(self):
        self.assertFalse(capability_validation.is_user_story_scenario(None))
        self.assertFalse(capability_validation.is_user_story_scenario(42))

    def test_case_insensitive(self):
        self.assertTrue(
            capability_validation.is_user_story_scenario(
                "AS A developer, I WANT to validate email, SO THAT signups are clean."
            )
        )


def write_use_case_contract(tmp_dir: str, scenario, namespace="core", cap_id="example-capability", version="1.0.0") -> Path:
    contract = {
        "id": cap_id,
        "namespace": namespace,
        "owner": {"team": "platform"},
        "version": version,
        "use_cases": [{"scenario": scenario, "input_example": {}, "output_example": {}, "happy": True}],
    }
    return write_contract(tmp_dir, contract, namespace=namespace, cap_id=cap_id, version=version)


class CheckNewScenarioFormatTests(unittest.TestCase):
    """check_new_scenario_format is the per-file check
    check_new_scenarios_are_user_stories (git-diff based, only run against
    newly-added contract.json files in a PR -- see its docstring for why it
    must never run against the whole historical tree) calls per path."""

    def test_valid_user_story_scenario_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_use_case_contract(
                tmp, "As a developer, I want to validate an email address, so that signups are clean."
            )
            errors: list = []
            capability_validation.check_new_scenario_format(path, errors)
            self.assertEqual(errors, [])

    def test_plain_declarative_scenario_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_use_case_contract(tmp, "A well-formed address is accepted.")
            errors: list = []
            capability_validation.check_new_scenario_format(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("contract.scenario_not_user_story", codes)

    def test_contract_without_use_cases_is_not_flagged(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_contract(tmp, valid_contract())
            errors: list = []
            capability_validation.check_new_scenario_format(path, errors)
            self.assertEqual(errors, [])


if __name__ == "__main__":
    unittest.main()
