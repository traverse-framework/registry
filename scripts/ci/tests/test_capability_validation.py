#!/usr/bin/env python3
"""Tests for specs/006-public-scope-and-identity FR-002/FR-003/FR-004 enforcement
in scripts/ci/capability_validation.py (registry issue #22).

Run with: python3 -m unittest scripts/ci/tests/test_capability_validation.py
"""

import importlib.util
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

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


class ServiceTypeValidationTests(unittest.TestCase):
    """traverse-framework/traverse spec 014-service-type-taxonomy: a closed
    enum registry does not own or extend, validated whole-tree (unlike
    use_cases[].scenario/persona_ref) since every already-published contract
    already conforms."""

    def test_known_service_type_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            contract = valid_contract()
            contract["service_type"] = "subscribable"
            path = write_contract(tmp, contract)
            errors: list = []
            capability_validation.validate_contract(path, errors)
            self.assertEqual(errors, [])

    def test_unknown_service_type_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            contract = valid_contract()
            contract["service_type"] = "eventual"
            path = write_contract(tmp, contract)
            errors: list = []
            capability_validation.validate_contract(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("contract.invalid_service_type", codes)

    def test_missing_service_type_is_not_flagged(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_contract(tmp, valid_contract())
            errors: list = []
            capability_validation.validate_contract(path, errors)
            codes = [e["code"] for e in errors]
            self.assertNotIn("contract.invalid_service_type", codes)


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

    def test_contract_without_use_cases_is_not_flagged_by_scenario_format(self):
        """Scenario-format check stays silent when use_cases is absent;
        surface-coverage (decision-log entry 55 / #215) is what fails
        closed on missing use_cases for ADDED/CHANGED contracts."""
        with tempfile.TemporaryDirectory() as tmp:
            path = write_contract(tmp, valid_contract())
            errors: list = []
            capability_validation.check_new_scenario_format(path, errors)
            self.assertEqual(errors, [])


def valid_persona(persona_id="alpha-persona", version="1.0.0", distinguished_from=None):
    return {
        "id": persona_id,
        "version": version,
        "name": "Alpha Persona",
        "summary": "A summary.",
        "description": "A fuller description.",
        "distinguished_from": distinguished_from if distinguished_from is not None else [],
    }


def write_persona(tmp_dir: str, persona: dict, persona_id="alpha-persona", version="1.0.0") -> Path:
    path = Path(tmp_dir) / "personas" / persona_id / version / "persona.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(persona))
    return path


class PersonaValidationTests(unittest.TestCase):
    """registry#177 follow-on, spec 017-persona-registry, decision-log
    entry 53: personas/ is a real governed content type, validated
    unconditionally (whole-tree) unlike the diff-based persona_ref check
    below -- this schema was correct from the very first persona."""

    def test_valid_persona_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_persona(tmp, valid_persona())
            errors: list = []
            capability_validation.validate_persona(path, errors)
            self.assertEqual(errors, [])

    def test_missing_required_field_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            persona = valid_persona()
            del persona["description"]
            path = write_persona(tmp, persona)
            errors: list = []
            capability_validation.validate_persona(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("persona.missing_required_field", codes)

    def test_id_mismatch_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            persona = valid_persona(persona_id="alpha-persona")
            persona["id"] = "different-persona"
            path = write_persona(tmp, persona, persona_id="alpha-persona")
            errors: list = []
            capability_validation.validate_persona(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("persona.id_mismatch", codes)

    def test_invalid_semver_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            persona = valid_persona(version="not-a-version")
            path = write_persona(tmp, persona, version="not-a-version")
            errors: list = []
            capability_validation.validate_persona(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("persona.invalid_semver", codes)

    def test_malformed_distinguished_from_entry_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            persona = valid_persona(distinguished_from=[{"persona_id": "beta-persona"}])
            path = write_persona(tmp, persona)
            errors: list = []
            capability_validation.validate_persona(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("persona.invalid_distinguished_from_entry", codes)


class CheckPersonaDistinguishedFromResolvesTests(unittest.TestCase):
    def test_two_personas_with_valid_mutual_references_pass(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_persona(
                tmp,
                valid_persona("alpha-persona", distinguished_from=[{"persona_id": "beta-persona", "how": "different domain"}]),
                persona_id="alpha-persona",
            )
            write_persona(
                tmp,
                valid_persona("beta-persona", distinguished_from=[{"persona_id": "alpha-persona", "how": "different domain"}]),
                persona_id="beta-persona",
            )
            import os

            cwd = os.getcwd()
            try:
                os.chdir(tmp)
                errors: list = []
                capability_validation.check_persona_distinguished_from_resolves(errors)
                self.assertEqual(errors, [])
            finally:
                os.chdir(cwd)

    def test_empty_distinguished_from_rejected_when_other_personas_exist(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_persona(tmp, valid_persona("alpha-persona", distinguished_from=[]), persona_id="alpha-persona")
            write_persona(
                tmp,
                valid_persona("beta-persona", distinguished_from=[{"persona_id": "alpha-persona", "how": "x"}]),
                persona_id="beta-persona",
            )
            import os

            cwd = os.getcwd()
            try:
                os.chdir(tmp)
                errors: list = []
                capability_validation.check_persona_distinguished_from_resolves(errors)
                codes = [e["code"] for e in errors]
                self.assertIn("persona.empty_distinguished_from", codes)
            finally:
                os.chdir(cwd)

    def test_dangling_reference_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_persona(
                tmp,
                valid_persona("alpha-persona", distinguished_from=[{"persona_id": "nonexistent", "how": "x"}]),
                persona_id="alpha-persona",
            )
            import os

            cwd = os.getcwd()
            try:
                os.chdir(tmp)
                errors: list = []
                capability_validation.check_persona_distinguished_from_resolves(errors)
                codes = [e["code"] for e in errors]
                self.assertIn("persona.distinguished_from_unresolvable", codes)
            finally:
                os.chdir(cwd)


def write_use_case_contract_with_persona_ref(tmp_dir: str, persona_ref, namespace="core", cap_id="example-capability", version="1.0.0") -> Path:
    contract = {
        "id": cap_id,
        "namespace": namespace,
        "owner": {"team": "platform"},
        "version": version,
        "use_cases": [
            {
                "scenario": "As a developer, I want to validate an email address, so that signups are clean.",
                "input_example": {},
                "output_example": {},
                "happy": True,
                "persona_ref": persona_ref,
            }
        ],
    }
    return write_contract(tmp_dir, contract, namespace=namespace, cap_id=cap_id, version=version)


class CheckNewUseCasePersonaRefTests(unittest.TestCase):
    """check_new_use_case_persona_ref is the per-file check
    check_new_use_cases_have_persona_ref (git-diff based, same reasoning as
    check_new_scenario_format) calls per path."""

    def test_valid_persona_ref_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_use_case_contract_with_persona_ref(tmp, "alpha-persona")
            errors: list = []
            capability_validation.check_new_use_case_persona_ref(path, errors, {"alpha-persona"})
            self.assertEqual(errors, [])

    def test_missing_persona_ref_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_use_case_contract_with_persona_ref(tmp, None)
            errors: list = []
            capability_validation.check_new_use_case_persona_ref(path, errors, {"alpha-persona"})
            codes = [e["code"] for e in errors]
            self.assertIn("contract.use_case_missing_persona_ref", codes)

    def test_unregistered_persona_ref_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_use_case_contract_with_persona_ref(tmp, "nonexistent-persona")
            errors: list = []
            capability_validation.check_new_use_case_persona_ref(path, errors, {"alpha-persona"})
            codes = [e["code"] for e in errors]
            self.assertIn("contract.use_case_persona_ref_unresolvable", codes)

    def test_contract_without_use_cases_is_not_flagged_by_persona_ref(self):
        """persona_ref check stays silent when use_cases is absent;
        surface-coverage (decision-log entry 55 / #215) is what fails
        closed on missing use_cases for ADDED/CHANGED contracts."""
        with tempfile.TemporaryDirectory() as tmp:
            path = write_contract(tmp, valid_contract())
            errors: list = []
            capability_validation.check_new_use_case_persona_ref(path, errors, {"alpha-persona"})
            self.assertEqual(errors, [])


def write_action_enum_contract(tmp_dir: str, enum_values, covered_actions) -> Path:
    use_cases = [
        {
            "scenario": "As a developer, I want to exercise an action, so that coverage holds.",
            "input_example": {"action": action},
            "output_example": {"ok": True},
            "happy": True,
            "persona_ref": "alpha-persona",
        }
        for action in covered_actions
    ]
    contract = valid_contract()
    contract["inputs"] = {
        "schema": {
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": enum_values},
            },
        }
    }
    contract["use_cases"] = use_cases
    return write_contract(tmp_dir, contract)


class CheckNewActionEnumCoverageTests(unittest.TestCase):
    """check_new_action_enum_covered_by_use_cases implements traverse Spec
    102 FR-001 / registry#192 for newly-ADDED contracts."""

    def test_full_coverage_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_action_enum_contract(tmp, ["create", "edit"], ["create", "edit"])
            errors: list = []
            capability_validation.check_new_action_enum_covered_by_use_cases(path, errors)
            self.assertEqual(errors, [])

    def test_uncovered_enum_value_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_action_enum_contract(tmp, ["create", "resolve"], ["create"])
            errors: list = []
            capability_validation.check_new_action_enum_covered_by_use_cases(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("contract.action_enum_uncovered_by_use_cases", codes)
            self.assertTrue(any("resolve" in e["message"] for e in errors))

    def test_missing_action_enum_is_not_flagged(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_contract(tmp, valid_contract())
            errors: list = []
            capability_validation.check_new_action_enum_covered_by_use_cases(path, errors)
            self.assertEqual(errors, [])

    def test_non_string_enum_value_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            contract = valid_contract()
            contract["inputs"] = {
                "schema": {
                    "properties": {
                        "action": {"enum": ["create", 1]},
                    }
                }
            }
            path = write_contract(tmp, contract)
            errors: list = []
            capability_validation.check_new_action_enum_covered_by_use_cases(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("contract.action_enum_non_string", codes)


def write_surface_coverage_contract(
    tmp_dir: str,
    *,
    use_cases,
    input_schema=None,
    output_schema=None,
) -> Path:
    contract = valid_contract()
    if input_schema is not None:
        contract["inputs"] = {"schema": input_schema}
    if output_schema is not None:
        contract["outputs"] = {"schema": output_schema}
    if use_cases is not None:
        contract["use_cases"] = use_cases
    return write_contract(tmp_dir, contract)


class CheckNewUseCasesSurfaceCoverageTests(unittest.TestCase):
    """check_new_use_cases_surface_coverage implements decision-log entry 55 /
    registry#215 / traverse Spec 102 FR-001–FR-004 for newly ADDED/CHANGED
    contracts. Diff-based wrapper is check_new_contracts_use_cases_surface_coverage."""

    def test_contract_without_use_cases_is_flagged(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_contract(tmp, valid_contract())
            errors: list = []
            capability_validation.check_new_use_cases_surface_coverage(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("contract.missing_use_cases", codes)

    def test_empty_use_cases_is_flagged(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_surface_coverage_contract(tmp, use_cases=[])
            errors: list = []
            capability_validation.check_new_use_cases_surface_coverage(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("contract.missing_use_cases", codes)

    def test_full_surface_coverage_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_surface_coverage_contract(
                tmp,
                input_schema={
                    "type": "object",
                    "required": ["action", "message_config"],
                    "properties": {
                        "action": {"type": "string", "enum": ["create", "edit"]},
                        "message_config": {
                            "type": "object",
                            "properties": {
                                "tone": {
                                    "type": "string",
                                    "enum": ["friendly", "direct"],
                                }
                            },
                        },
                    },
                },
                output_schema={
                    "type": "object",
                    "properties": {
                        "reason_code": {
                            "type": "string",
                            "enum": ["ok", "invalid_input"],
                        },
                        "status": {"type": "string", "enum": ["accepted", "rejected"]},
                    },
                },
                use_cases=[
                    {
                        "scenario": "As a developer, I want to create, so that coverage holds.",
                        "input_example": {
                            "action": "create",
                            "message_config": {"tone": "friendly"},
                        },
                        "output_example": {
                            "reason_code": "ok",
                            "status": "accepted",
                        },
                        "happy": True,
                    },
                    {
                        "scenario": "As a developer, I want to edit, so that coverage holds.",
                        "input_example": {
                            "action": "edit",
                            "message_config": {"tone": "direct"},
                        },
                        "output_example": {
                            "reason_code": "invalid_input",
                            "status": "rejected",
                        },
                        "happy": False,
                    },
                ],
            )
            errors: list = []
            capability_validation.check_new_use_cases_surface_coverage(path, errors)
            self.assertEqual(errors, [])

    def test_uncovered_nested_input_enum_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_surface_coverage_contract(
                tmp,
                input_schema={
                    "properties": {
                        "message_config": {
                            "type": "object",
                            "properties": {
                                "tone": {
                                    "type": "string",
                                    "enum": ["friendly", "direct"],
                                }
                            },
                        }
                    }
                },
                use_cases=[
                    {
                        "scenario": "As a developer, I want friendly tone, so that coverage holds.",
                        "input_example": {"message_config": {"tone": "friendly"}},
                        "output_example": {"ok": True},
                        "happy": True,
                    }
                ],
            )
            errors: list = []
            capability_validation.check_new_use_cases_surface_coverage(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("contract.input_enum_uncovered_by_use_cases", codes)
            self.assertTrue(any("direct" in e["message"] for e in errors))

    def test_uncovered_required_input_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_surface_coverage_contract(
                tmp,
                input_schema={
                    "required": ["action", "title"],
                    "properties": {
                        "action": {"type": "string", "enum": ["create"]},
                        "title": {"type": "string"},
                    },
                },
                use_cases=[
                    {
                        "scenario": "As a developer, I want to create, so that coverage holds.",
                        "input_example": {"action": "create"},
                        "output_example": {"ok": True},
                        "happy": True,
                    }
                ],
            )
            errors: list = []
            capability_validation.check_new_use_cases_surface_coverage(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("contract.required_input_uncovered_by_use_cases", codes)
            self.assertTrue(any("title" in e["message"] for e in errors))

    def test_uncovered_output_reason_code_enum_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_surface_coverage_contract(
                tmp,
                output_schema={
                    "properties": {
                        "reason_code": {
                            "type": "string",
                            "enum": ["ok", "invalid_input"],
                        }
                    }
                },
                use_cases=[
                    {
                        "scenario": "As a developer, I want success, so that coverage holds.",
                        "input_example": {},
                        "output_example": {"reason_code": "ok"},
                        "happy": True,
                    }
                ],
            )
            errors: list = []
            capability_validation.check_new_use_cases_surface_coverage(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("contract.output_enum_uncovered_by_use_cases", codes)
            self.assertTrue(any("invalid_input" in e["message"] for e in errors))

    def test_uncovered_output_status_enum_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_surface_coverage_contract(
                tmp,
                output_schema={
                    "properties": {
                        "status": {"type": "string", "enum": ["accepted", "rejected"]}
                    }
                },
                use_cases=[
                    {
                        "scenario": "As a developer, I want acceptance, so that coverage holds.",
                        "input_example": {},
                        "output_example": {"status": "accepted"},
                        "happy": True,
                    }
                ],
            )
            errors: list = []
            capability_validation.check_new_use_cases_surface_coverage(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("contract.output_enum_uncovered_by_use_cases", codes)
            self.assertTrue(any("rejected" in e["message"] for e in errors))


def write_artifact_contract(
    tmp_dir: str,
    artifact,
    namespace="core",
    cap_id="example-capability",
    version="1.0.0",
) -> Path:
    contract = valid_contract()
    contract["id"] = cap_id
    contract["namespace"] = namespace
    contract["version"] = version
    if artifact is not None:
        contract["artifact"] = artifact
    return write_contract(tmp_dir, contract, namespace=namespace, cap_id=cap_id, version=version)


class CheckNewContractArtifactReferenceTests(unittest.TestCase):
    """check_new_contract_artifact_reference implements spec 001 FR-007 /
    spec 007 FR-001 / registry#187 for newly-ADDED contracts."""

    VALID_URL = (
        "https://github.com/traverse-framework/registry/releases/download/"
        "artifacts/example-capability-1.0.0/example-capability.wasm"
    )

    def test_valid_artifact_reference_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_artifact_contract(
                tmp,
                {"digest": "sha256:" + ("a" * 64), "url": self.VALID_URL},
            )
            errors: list = []
            capability_validation.check_new_contract_artifact_reference(path, errors)
            self.assertEqual(errors, [])

    def test_missing_artifact_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_artifact_contract(tmp, None)
            errors: list = []
            capability_validation.check_new_contract_artifact_reference(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("contract.missing_artifact_reference", codes)

    def test_artifact_missing_digest_or_url_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_artifact_contract(tmp, {"digest": "sha256:" + ("a" * 64)})
            errors: list = []
            capability_validation.check_new_contract_artifact_reference(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("contract.missing_artifact_reference", codes)

    def test_non_sha256_digest_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_artifact_contract(
                tmp,
                {"digest": "md5:deadbeef", "url": self.VALID_URL},
            )
            errors: list = []
            capability_validation.check_new_contract_artifact_reference(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("contract.invalid_digest_format", codes)

    def test_non_artifacts_release_url_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_artifact_contract(
                tmp,
                {
                    "digest": "sha256:" + ("a" * 64),
                    "url": "https://example.invalid/artifact.wasm",
                },
            )
            errors: list = []
            capability_validation.check_new_contract_artifact_reference(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("contract.invalid_artifact_url", codes)


class ExpectedCapabilitySrcCrateTests(unittest.TestCase):
    def test_dots_replaced_with_dashes(self):
        self.assertEqual(
            capability_validation.expected_capability_src_crate("artifact.revision-create"),
            "artifact-revision-create",
        )

    def test_no_dots_unchanged(self):
        self.assertEqual(
            capability_validation.expected_capability_src_crate("validate-luhn"),
            "validate-luhn",
        )


class CheckNewContractTestCoverageTests(unittest.TestCase):
    """specs/018-capability-test-coverage FR-001 through FR-003."""

    def _write_contract(self, tmp: Path, capability_id: str) -> Path:
        path = Path(tmp) / "capabilities" / "example" / capability_id / "1.0.0" / "contract.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps({"id": capability_id}))
        return path

    def _write_crate(self, tmp: Path, crate_name: str) -> None:
        crate_dir = Path(tmp) / "capability-src" / crate_name
        crate_dir.mkdir(parents=True, exist_ok=True)
        (crate_dir / "Cargo.toml").write_text("[package]\nname = \"x\"\n")

    def _cov_result(self, functions=100.0, lines=100.0, regions=100.0, returncode=0, stderr=""):
        payload = json.dumps(
            {
                "data": [
                    {
                        "totals": {
                            "functions": {"percent": functions},
                            "lines": {"percent": lines},
                            "regions": {"percent": regions},
                        }
                    }
                ]
            }
        )
        return type(
            "Result",
            (),
            {"returncode": returncode, "stdout": payload, "stderr": stderr},
        )()

    def test_missing_crate_directory_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write_contract(tmp, "example.new-capability")
            errors: list = []
            cwd = os.getcwd()
            try:
                os.chdir(tmp)
                capability_validation.check_new_contract_test_coverage(path, errors)
            finally:
                os.chdir(cwd)
            codes = [e["code"] for e in errors]
            self.assertIn("capability.missing_test_coverage_source", codes)

    def test_full_coverage_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write_contract(tmp, "example.new-capability")
            self._write_crate(tmp, "example-new-capability")
            errors: list = []
            cwd = os.getcwd()
            try:
                os.chdir(tmp)
                with patch("capability_validation.subprocess.run", return_value=self._cov_result()):
                    capability_validation.check_new_contract_test_coverage(path, errors)
            finally:
                os.chdir(cwd)
            self.assertEqual(errors, [])

    def test_insufficient_lines_coverage_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write_contract(tmp, "example.new-capability")
            self._write_crate(tmp, "example-new-capability")
            errors: list = []
            cwd = os.getcwd()
            try:
                os.chdir(tmp)
                result = self._cov_result(functions=100.0, lines=80.0, regions=96.0)
                with patch("capability_validation.subprocess.run", return_value=result):
                    capability_validation.check_new_contract_test_coverage(path, errors)
            finally:
                os.chdir(cwd)
            codes = [e["code"] for e in errors]
            self.assertIn("capability.insufficient_test_coverage", codes)

    def test_incomplete_function_coverage_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write_contract(tmp, "example.new-capability")
            self._write_crate(tmp, "example-new-capability")
            errors: list = []
            cwd = os.getcwd()
            try:
                os.chdir(tmp)
                result = self._cov_result(functions=92.0, lines=99.0, regions=99.0)
                with patch("capability_validation.subprocess.run", return_value=result):
                    capability_validation.check_new_contract_test_coverage(path, errors)
            finally:
                os.chdir(cwd)
            codes = [e["code"] for e in errors]
            self.assertIn("capability.insufficient_test_coverage", codes)

    def test_build_or_test_failure_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write_contract(tmp, "example.new-capability")
            self._write_crate(tmp, "example-new-capability")
            errors: list = []
            cwd = os.getcwd()
            try:
                os.chdir(tmp)
                result = self._cov_result(returncode=1, stderr="error: could not compile")
                with patch("capability_validation.subprocess.run", return_value=result):
                    capability_validation.check_new_contract_test_coverage(path, errors)
            finally:
                os.chdir(cwd)
            codes = [e["code"] for e in errors]
            self.assertIn("capability.test_coverage_build_or_test_failed", codes)

    def test_boundary_at_exactly_ninety_five_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write_contract(tmp, "example.new-capability")
            self._write_crate(tmp, "example-new-capability")
            errors: list = []
            cwd = os.getcwd()
            try:
                os.chdir(tmp)
                result = self._cov_result(functions=100.0, lines=95.0, regions=95.0)
                with patch("capability_validation.subprocess.run", return_value=result):
                    capability_validation.check_new_contract_test_coverage(path, errors)
            finally:
                os.chdir(cwd)
            self.assertEqual(errors, [])


class CheckEccaCapabilityInventoryCoverageTests(unittest.TestCase):
    """Spec 534 FR-020 / registry#253: inventory must cover every published capability."""

    def _write_inventory(self, tmp: Path, entries: list) -> Path:
        inv_path = tmp / "contracts" / "governance" / "ecca-capability-inventory.json"
        inv_path.parent.mkdir(parents=True, exist_ok=True)
        inv_path.write_text(
            json.dumps(
                {
                    "kind": "ecca_capability_inventory",
                    "schema_version": "1.0.0",
                    "capabilities": entries,
                }
            )
        )
        return inv_path

    def test_complete_inventory_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_contract(
                tmp,
                {
                    "id": "example-capability",
                    "namespace": "core",
                    "owner": {"team": "platform"},
                    "version": "1.0.0",
                },
            )
            product = root / "events" / "core" / "example.event" / "1.0.0" / "product.json"
            product.parent.mkdir(parents=True, exist_ok=True)
            product.write_text("{}")
            self._write_inventory(
                root,
                [
                    {
                        "capability_id": "example-capability",
                        "published_versions": ["1.0.0"],
                        "path": "capabilities/core/example-capability/1.0.0/contract.json",
                        "classification": "no-event-required",
                        "evidence": "memory_only; empty emits",
                    }
                ],
            )
            errors: list = []
            cwd = Path.cwd()
            try:
                import os

                os.chdir(root)
                capability_validation.check_ecca_capability_inventory_coverage(errors)
            finally:
                os.chdir(cwd)
            self.assertEqual(errors, [])

    def test_missing_inventory_entry_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_contract(
                tmp,
                {
                    "id": "example-capability",
                    "namespace": "core",
                    "owner": {"team": "platform"},
                    "version": "1.0.0",
                },
            )
            self._write_inventory(root, [])
            errors: list = []
            cwd = Path.cwd()
            try:
                import os

                os.chdir(root)
                capability_validation.check_ecca_capability_inventory_coverage(errors)
            finally:
                os.chdir(cwd)
            codes = [e["code"] for e in errors]
            self.assertIn("inventory.unpublished_capability_unclassified", codes)


def _valid_signature_record():
    return {
        "scheme": "ed25519",
        "public_key_hex": "aa" * 32,
        "signature_hex": "bb" * 64,
        "sigstore_bundle_ref": None,
        "signed_at": "2026-08-28T00:00:00Z",
    }


class SignatureFileShapeTests(unittest.TestCase):
    """specs/007-artifact-hosting amendment FR-007/FR-008/FR-012 (registry#334)."""

    def _write(self, tmp, sig, *, with_artifact=True):
        version_dir = Path(tmp) / "capabilities" / "core" / "core.example" / "1.0.0"
        version_dir.mkdir(parents=True, exist_ok=True)
        contract = valid_contract()
        contract["id"] = "core.example"
        if with_artifact:
            contract["artifact"] = {
                "digest": "sha256:" + "0" * 64,
                "url": "https://github.com/traverse-framework/registry/releases/download/artifacts/x-1.0.0/x.wasm",
            }
        (version_dir / "contract.json").write_text(json.dumps(contract))
        sig_path = version_dir / "signature.json"
        sig_path.write_text(json.dumps(sig))
        return sig_path

    def test_valid_signature_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            errors: list = []
            capability_validation.validate_signature_file(self._write(tmp, _valid_signature_record()), errors)
            self.assertEqual(errors, [])

    def test_missing_field_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            sig = _valid_signature_record()
            del sig["signed_at"]
            errors: list = []
            capability_validation.validate_signature_file(self._write(tmp, sig), errors)
            self.assertIn("signature.missing_fields", [e["code"] for e in errors])

    def test_non_ed25519_scheme_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            sig = _valid_signature_record()
            sig["scheme"] = "sigstore"
            errors: list = []
            capability_validation.validate_signature_file(self._write(tmp, sig), errors)
            self.assertIn("signature.bad_scheme", [e["code"] for e in errors])

    def test_non_null_sigstore_ref_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            sig = _valid_signature_record()
            sig["sigstore_bundle_ref"] = "ref://x"
            errors: list = []
            capability_validation.validate_signature_file(self._write(tmp, sig), errors)
            self.assertIn("signature.bad_sigstore_ref", [e["code"] for e in errors])

    def test_bad_key_length_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            sig = _valid_signature_record()
            sig["public_key_hex"] = "aa" * 16
            errors: list = []
            capability_validation.validate_signature_file(self._write(tmp, sig), errors)
            self.assertIn("signature.bad_public_key", [e["code"] for e in errors])

    def test_signature_without_artifact_field_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            errors: list = []
            capability_validation.validate_signature_file(
                self._write(tmp, _valid_signature_record(), with_artifact=False), errors
            )
            self.assertIn("signature.unexpected", [e["code"] for e in errors])


class SignatureCompletenessTests(unittest.TestCase):
    def _tree(self, tmp, *, signed, deprecated=False, enforced=False):
        version_dir = Path(tmp) / "capabilities" / "core" / "core.example" / "1.0.0"
        version_dir.mkdir(parents=True, exist_ok=True)
        contract = valid_contract()
        contract["id"] = "core.example"
        contract["artifact"] = {
            "digest": "sha256:" + "0" * 64,
            "url": "https://github.com/traverse-framework/registry/releases/download/artifacts/x-1.0.0/x.wasm",
        }
        (version_dir / "contract.json").write_text(json.dumps(contract))
        if signed:
            (version_dir / "signature.json").write_text(json.dumps(_valid_signature_record()))
        if deprecated:
            (version_dir / "deprecated.json").write_text(json.dumps({"reason": "x"}))
        if enforced:
            (Path(tmp) / "capabilities" / ".signatures-enforced").write_text("enforced\n")

    def _run(self, tmp):
        errors: list = []
        cwd = os.getcwd()
        try:
            os.chdir(tmp)
            capability_validation.check_signature_siblings(errors)
        finally:
            os.chdir(cwd)
        return errors

    def test_missing_signature_is_advisory_by_default(self):
        with tempfile.TemporaryDirectory() as tmp:
            self._tree(tmp, signed=False)
            self.assertEqual(self._run(tmp), [])

    def test_missing_signature_fails_when_enforced(self):
        with tempfile.TemporaryDirectory() as tmp:
            self._tree(tmp, signed=False, enforced=True)
            self.assertIn("signature.missing", [e["code"] for e in self._run(tmp)])

    def test_deprecated_version_never_requires_signature(self):
        with tempfile.TemporaryDirectory() as tmp:
            self._tree(tmp, signed=False, deprecated=True, enforced=True)
            self.assertEqual(self._run(tmp), [])

    def test_signed_version_passes_when_enforced(self):
        with tempfile.TemporaryDirectory() as tmp:
            self._tree(tmp, signed=True, enforced=True)
            self.assertEqual(self._run(tmp), [])


if __name__ == "__main__":
    unittest.main()
