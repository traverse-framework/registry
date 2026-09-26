#!/usr/bin/env python3
"""Tests for specs/006-public-scope-and-identity FR-002/FR-003/FR-004 enforcement
in scripts/ci/capability_validation.py (registry issue #22).

Run with: python3 -m unittest scripts/ci/tests/test_capability_validation.py
"""

import hashlib
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


class AiObjectValidationTests(unittest.TestCase):
    """spec 001 FR-017: optional `ai` object marking a model-backed capability."""

    def _codes(self, ai):
        with tempfile.TemporaryDirectory() as tmp:
            contract = valid_contract()
            contract["ai"] = ai
            path = write_contract(tmp, contract)
            errors: list = []
            capability_validation.validate_contract(path, errors)
            return [e["code"] for e in errors]

    def test_absent_ai_is_not_flagged(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_contract(tmp, valid_contract())
            errors: list = []
            capability_validation.validate_contract(path, errors)
            self.assertEqual(errors, [])

    def test_model_backed_false_without_models_passes(self):
        self.assertNotIn("contract.invalid_ai", self._codes({"model_backed": False}))

    def test_model_backed_true_with_models_passes(self):
        self.assertNotIn(
            "contract.invalid_ai",
            self._codes({"model_backed": True, "models": ["minishlab/potion-base-32M"]}),
        )

    def test_model_backed_true_without_models_is_rejected(self):
        self.assertIn("contract.invalid_ai", self._codes({"model_backed": True}))

    def test_model_backed_true_with_empty_models_is_rejected(self):
        self.assertIn("contract.invalid_ai", self._codes({"model_backed": True, "models": []}))

    def test_non_boolean_model_backed_is_rejected(self):
        self.assertIn("contract.invalid_ai", self._codes({"model_backed": "yes"}))

    def test_non_string_models_entry_is_rejected(self):
        self.assertIn(
            "contract.invalid_ai",
            self._codes({"model_backed": True, "models": ["ok", 3]}),
        )

    def test_ai_not_an_object_is_rejected(self):
        self.assertIn("contract.invalid_ai", self._codes(["model_backed"]))

    # --- spec 001 FR-017 amendment (decision-log entry 124, registry#571):
    # object-shaped ai.models, accepted alongside the legacy string[] shape.

    def _object_model_ref(self, **overrides):
        base = {
            "id": "minishlab/potion-base-32M",
            "spdx_expression": "MIT",
            "attribution_required": True,
            "huggingface_id": "minishlab/potion-base-32M",
            "revision": "abc1234",
        }
        base.update(overrides)
        return base

    def test_object_shaped_models_with_hf_pin_passes(self):
        self.assertEqual(
            self._codes({"model_backed": True, "models": [self._object_model_ref()]}),
            [],
        )

    def test_object_shaped_models_with_source_url_passes(self):
        ref = self._object_model_ref(source_url="https://github.com/snakers4/silero-vad")
        del ref["huggingface_id"]
        del ref["revision"]
        self.assertEqual(self._codes({"model_backed": True, "models": [ref]}), [])

    def test_object_shaped_models_missing_id_is_rejected(self):
        ref = self._object_model_ref()
        del ref["id"]
        self.assertIn(
            "contract.invalid_ai", self._codes({"model_backed": True, "models": [ref]})
        )

    def test_object_shaped_models_missing_spdx_expression_is_rejected(self):
        ref = self._object_model_ref()
        del ref["spdx_expression"]
        self.assertIn(
            "contract.invalid_ai", self._codes({"model_backed": True, "models": [ref]})
        )

    def test_object_shaped_models_non_boolean_attribution_required_is_rejected(self):
        ref = self._object_model_ref(attribution_required="yes")
        self.assertIn(
            "contract.invalid_ai", self._codes({"model_backed": True, "models": [ref]})
        )

    def test_object_shaped_models_without_hf_pin_or_source_url_is_rejected(self):
        ref = self._object_model_ref()
        del ref["huggingface_id"]
        del ref["revision"]
        self.assertIn(
            "contract.invalid_ai", self._codes({"model_backed": True, "models": [ref]})
        )

    def test_object_shaped_models_partial_hf_pin_is_rejected(self):
        # huggingface_id without revision is not a real pin (decision 124:
        # "pin, don't live-fetch" -- an unpinned id has no fixed provenance).
        ref = self._object_model_ref()
        del ref["revision"]
        self.assertIn(
            "contract.invalid_ai", self._codes({"model_backed": True, "models": [ref]})
        )

    def test_object_shaped_models_invalid_spdx_expression_is_rejected(self):
        ref = self._object_model_ref(spdx_expression="Not A Real License")
        self.assertIn(
            "contract.invalid_licensing_spdx",
            self._codes({"model_backed": True, "models": [ref]}),
        )

    def test_object_shaped_models_unsafe_source_url_is_rejected(self):
        ref = self._object_model_ref(source_url="http://example.com/model")
        del ref["huggingface_id"]
        del ref["revision"]
        self.assertIn(
            "contract.invalid_ai", self._codes({"model_backed": True, "models": [ref]})
        )

    def test_object_shaped_models_non_string_copyright_is_rejected(self):
        ref = self._object_model_ref(copyright=2026)
        self.assertIn(
            "contract.invalid_ai", self._codes({"model_backed": True, "models": [ref]})
        )

    def test_legacy_string_models_still_passes(self):
        # Grandfathered forever on already-published contracts (decision 124).
        self.assertEqual(
            self._codes({"model_backed": True, "models": ["dslim/distilbert-NER"]}),
            [],
        )


def valid_licensing(**overrides):
    base = {
        "spdx_expression": "MIT",
        "commercial_use": "allowed",
        "redistribution": "allowed",
        "attribution_required": True,
        "verification": {"status": "maintainer-declared"},
    }
    base.update(overrides)
    return base


class LicensingValidationTests(unittest.TestCase):
    """specs/025-capability-licensing-metadata — optional licensing block."""

    def _codes(self, licensing):
        with tempfile.TemporaryDirectory() as tmp:
            contract = valid_contract()
            if licensing is not None:
                contract["licensing"] = licensing
            path = write_contract(tmp, contract)
            errors: list = []
            capability_validation.validate_contract(path, errors)
            return [e["code"] for e in errors]

    def test_absent_licensing_passes(self):
        self.assertEqual(self._codes(None), [])

    def test_valid_mit_passes(self):
        self.assertEqual(self._codes(valid_licensing()), [])

    def test_spdx_expression_or_passes(self):
        self.assertEqual(
            self._codes(valid_licensing(spdx_expression="MIT OR Apache-2.0")),
            [],
        )

    def test_forbidden_conditional_unknown_pass(self):
        for rights in ("forbidden", "conditional", "unknown"):
            codes = self._codes(
                valid_licensing(commercial_use=rights, redistribution=rights)
            )
            self.assertEqual(codes, [], msg=rights)

    def test_invalid_rights_enum_rejected(self):
        self.assertIn(
            "contract.invalid_licensing",
            self._codes(valid_licensing(commercial_use="yes")),
        )

    def test_missing_required_subfields_rejected(self):
        self.assertIn(
            "contract.invalid_licensing",
            self._codes({"spdx_expression": "MIT"}),
        )

    def test_reserved_verification_status_rejected(self):
        self.assertIn(
            "contract.invalid_licensing_verification",
            self._codes(
                valid_licensing(verification={"status": "registry-reviewed"})
            ),
        )

    def test_bad_source_url_rejected(self):
        self.assertIn(
            "contract.invalid_licensing_url",
            self._codes(valid_licensing(source_url="http://example.com/LICENSE")),
        )
        self.assertIn(
            "contract.invalid_licensing_url",
            self._codes(valid_licensing(source_url="/etc/passwd")),
        )
        self.assertIn(
            "contract.invalid_licensing_url",
            self._codes(
                valid_licensing(source_url="https://user:pass@example.com/LICENSE")
            ),
        )

    def test_licenseref_without_evidence_rejected(self):
        self.assertIn(
            "contract.licensing_licenseref_needs_evidence",
            self._codes(valid_licensing(spdx_expression="LicenseRef-Custom")),
        )

    def test_licenseref_with_evidence_url_passes(self):
        self.assertEqual(
            self._codes(
                valid_licensing(
                    spdx_expression="LicenseRef-Custom",
                    verification={
                        "status": "maintainer-declared",
                        "evidence_url": "https://example.com/LICENSE",
                    },
                )
            ),
            [],
        )

    def test_licenseref_with_license_files_passes(self):
        self.assertEqual(
            self._codes(
                valid_licensing(
                    spdx_expression="LicenseRef-Custom",
                    license_files=["LICENSE"],
                )
            ),
            [],
        )

    def test_unlicensed_with_redistribution_allowed_contradiction(self):
        self.assertIn(
            "contract.licensing_contradiction",
            self._codes(
                valid_licensing(
                    spdx_expression="UNLICENSED",
                    redistribution="allowed",
                )
            ),
        )

    def test_proprietary_with_redistribution_allowed_contradiction(self):
        self.assertIn(
            "contract.licensing_contradiction",
            self._codes(
                valid_licensing(
                    spdx_expression="LicenseRef-Proprietary",
                    redistribution="allowed",
                    license_files=["LICENSE"],
                )
            ),
        )

    def test_unlicensed_with_redistribution_forbidden_passes(self):
        self.assertEqual(
            self._codes(
                valid_licensing(
                    spdx_expression="UNLICENSED",
                    commercial_use="forbidden",
                    redistribution="forbidden",
                )
            ),
            [],
        )

    def test_malformed_spdx_rejected(self):
        self.assertIn(
            "contract.invalid_licensing_spdx",
            self._codes(valid_licensing(spdx_expression="MIT AND AND Apache-2.0")),
        )

    def test_unknown_non_licenseref_key_rejected(self):
        self.assertIn(
            "contract.invalid_licensing_spdx",
            self._codes(valid_licensing(spdx_expression="NotARealLicense-1.0")),
        )


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


class _FakeArtifactResponse:
    """Minimal stand-in for the object urllib.request.urlopen's context
    manager yields, supporting the chunked .read(n) loop
    check_new_contract_artifact_fetchable uses."""

    def __init__(self, body: bytes):
        self._body = body
        self._pos = 0

    def read(self, n=-1):
        if n is None or n < 0:
            chunk = self._body[self._pos :]
            self._pos = len(self._body)
            return chunk
        chunk = self._body[self._pos : self._pos + n]
        self._pos += len(chunk)
        return chunk

    def __enter__(self):
        return self

    def __exit__(self, *exc_info):
        return False


class CheckNewContractArtifactFetchableTests(unittest.TestCase):
    """registry#510: a newly-ADDED contract.json's artifact.url must actually
    resolve, and its bytes must match artifact.digest, before merge."""

    VALID_URL = (
        "https://github.com/traverse-framework/registry/releases/download/"
        "artifacts/example-capability-1.0.0/example-capability.wasm"
    )

    def _digest_for(self, body: bytes) -> str:
        return "sha256:" + hashlib.sha256(body).hexdigest()

    def test_no_artifact_is_skipped(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_artifact_contract(tmp, None)
            errors: list = []
            with patch("capability_validation.urllib.request.urlopen") as mock_open:
                capability_validation.check_new_contract_artifact_fetchable(path, errors)
            mock_open.assert_not_called()
            self.assertEqual(errors, [])

    def test_malformed_digest_is_skipped(self):
        # check_new_contract_artifact_reference already rejects this shape;
        # this check has nothing well-formed to fetch/compare against.
        with tempfile.TemporaryDirectory() as tmp:
            path = write_artifact_contract(tmp, {"digest": "md5:deadbeef", "url": self.VALID_URL})
            errors: list = []
            with patch("capability_validation.urllib.request.urlopen") as mock_open:
                capability_validation.check_new_contract_artifact_fetchable(path, errors)
            mock_open.assert_not_called()
            self.assertEqual(errors, [])

    def test_matching_digest_passes(self):
        body = b"fake wasm bytes"
        with tempfile.TemporaryDirectory() as tmp:
            path = write_artifact_contract(
                tmp, {"digest": self._digest_for(body), "url": self.VALID_URL}
            )
            errors: list = []
            with patch(
                "capability_validation.urllib.request.urlopen",
                return_value=_FakeArtifactResponse(body),
            ):
                capability_validation.check_new_contract_artifact_fetchable(path, errors)
            self.assertEqual(errors, [])

    def test_digest_mismatch_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_artifact_contract(
                tmp, {"digest": "sha256:" + ("a" * 64), "url": self.VALID_URL}
            )
            errors: list = []
            with patch(
                "capability_validation.urllib.request.urlopen",
                return_value=_FakeArtifactResponse(b"fake wasm bytes"),
            ):
                capability_validation.check_new_contract_artifact_fetchable(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("contract.artifact_digest_mismatch", codes)

    def test_unreachable_url_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_artifact_contract(
                tmp, {"digest": "sha256:" + ("a" * 64), "url": self.VALID_URL}
            )
            errors: list = []
            with patch(
                "capability_validation.urllib.request.urlopen",
                side_effect=capability_validation.urllib.error.URLError("404"),
            ):
                capability_validation.check_new_contract_artifact_fetchable(path, errors)
            codes = [e["code"] for e in errors]
            self.assertIn("contract.artifact_url_unreachable", codes)


class CheckNewContractAuthoringMethodTests(unittest.TestCase):
    """check_new_contract_authoring_method implements spec
    023-authoring-assurance FR-001/FR-003 for newly ADDED or CHANGED
    contracts. Diff-based only -- pre-023 immutable publishes have no
    `authoring` block and must never be retro-flagged."""

    LLM_AUDIT = {
        "source_revision": "abc1234",
        "test_evidence": "https://github.com/traverse-framework/registry/actions/runs/1",
        "review": "approved by @owner in PR #999",
    }

    def _contract(self, authoring):
        c = valid_contract()
        if authoring is not None:
            c["authoring"] = authoring
        return c

    def test_human_method_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_contract(tmp, self._contract({"method": "human"}))
            errors: list = []
            capability_validation.check_new_contract_authoring_method(path, errors)
            self.assertEqual(errors, [])

    def test_llm_assisted_with_full_audit_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_contract(
                tmp, self._contract({"method": "llm-assisted", **self.LLM_AUDIT})
            )
            errors: list = []
            capability_validation.check_new_contract_authoring_method(path, errors)
            self.assertEqual(errors, [])

    def test_missing_authoring_block_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_contract(tmp, self._contract(None))
            errors: list = []
            capability_validation.check_new_contract_authoring_method(path, errors)
            self.assertIn(
                "contract.missing_authoring_method", [e["code"] for e in errors]
            )

    def test_authoring_without_method_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_contract(tmp, self._contract({"note": "no method here"}))
            errors: list = []
            capability_validation.check_new_contract_authoring_method(path, errors)
            self.assertIn(
                "contract.missing_authoring_method", [e["code"] for e in errors]
            )

    def test_unknown_method_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_contract(tmp, self._contract({"method": "ai-generated"}))
            errors: list = []
            capability_validation.check_new_contract_authoring_method(path, errors)
            self.assertIn(
                "contract.invalid_authoring_method", [e["code"] for e in errors]
            )

    def test_llm_assisted_missing_audit_field_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            partial = {"method": "llm-assisted", **self.LLM_AUDIT}
            del partial["review"]
            path = write_contract(tmp, self._contract(partial))
            errors: list = []
            capability_validation.check_new_contract_authoring_method(path, errors)
            self.assertIn(
                "contract.missing_authoring_audit", [e["code"] for e in errors]
            )

    def test_human_method_needs_no_audit_fields(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_contract(tmp, self._contract({"method": "human"}))
            errors: list = []
            capability_validation.check_new_contract_authoring_method(path, errors)
            self.assertEqual(
                [e for e in errors if e["code"] == "contract.missing_authoring_audit"], []
            )


class CheckNewContractLicensingTests(unittest.TestCase):
    """check_new_contract_licensing implements spec 025 FR-010 activation."""

    def test_present_licensing_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            contract = valid_contract()
            contract["licensing"] = valid_licensing()
            path = write_contract(tmp, contract)
            errors: list = []
            capability_validation.check_new_contract_licensing(path, errors)
            self.assertEqual(errors, [])

    def test_missing_licensing_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_contract(tmp, valid_contract())
            errors: list = []
            capability_validation.check_new_contract_licensing(path, errors)
            self.assertIn("contract.missing_licensing", [e["code"] for e in errors])


class CheckNewContractAiModelsObjectShapeTests(unittest.TestCase):
    """check_new_contract_ai_models_object_shape implements spec 001 FR-017's
    amendment (decision-log entry 124, registry#571): a newly-ADDED contract
    with ai.model_backed: true must use the object-shaped ai.models, not the
    legacy string[] shape."""

    def _object_model_ref(self):
        return {
            "id": "minishlab/potion-base-32M",
            "spdx_expression": "MIT",
            "attribution_required": True,
            "huggingface_id": "minishlab/potion-base-32M",
            "revision": "abc1234",
        }

    def test_object_shaped_models_on_new_contract_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            contract = valid_contract()
            contract["ai"] = {"model_backed": True, "models": [self._object_model_ref()]}
            path = write_contract(tmp, contract)
            errors: list = []
            capability_validation.check_new_contract_ai_models_object_shape(path, errors)
            self.assertEqual(errors, [])

    def test_legacy_string_models_on_new_contract_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            contract = valid_contract()
            contract["ai"] = {"model_backed": True, "models": ["dslim/distilbert-NER"]}
            path = write_contract(tmp, contract)
            errors: list = []
            capability_validation.check_new_contract_ai_models_object_shape(path, errors)
            self.assertIn(
                "contract.ai_models_legacy_shape_on_new_contract",
                [e["code"] for e in errors],
            )

    def test_non_model_backed_contract_is_not_flagged(self):
        with tempfile.TemporaryDirectory() as tmp:
            contract = valid_contract()
            contract["ai"] = {"model_backed": False}
            path = write_contract(tmp, contract)
            errors: list = []
            capability_validation.check_new_contract_ai_models_object_shape(path, errors)
            self.assertEqual(errors, [])

    def test_contract_without_ai_is_not_flagged(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_contract(tmp, valid_contract())
            errors: list = []
            capability_validation.check_new_contract_ai_models_object_shape(path, errors)
            self.assertEqual(errors, [])

    def test_missing_models_is_not_flagged_here(self):
        # Already reported by validate_contract's whole-tree ai check
        # (contract.invalid_ai); this forward gate only judges shape once a
        # non-empty models array is present.
        with tempfile.TemporaryDirectory() as tmp:
            contract = valid_contract()
            contract["ai"] = {"model_backed": True}
            path = write_contract(tmp, contract)
            errors: list = []
            capability_validation.check_new_contract_ai_models_object_shape(path, errors)
            self.assertEqual(errors, [])


class CheckNewContractRiskMetadataTests(unittest.TestCase):
    """check_new_contract_risk_metadata implements spec
    024-capability-risk-classification-adoption FR-001/FR-002 for newly ADDED
    or CHANGED contracts. Diff-based only -- the ~115 pre-024 immutable
    publishes have no `risk` block and must never be retro-flagged."""

    ELIGIBLE_RISK = {
        "effect_class": "pure_read",
        "determinism_class": "deterministic",
        "data_flow": {"egress_policy": "denied"},
        "reliability": {
            "idempotency_required": False,
            "retryable": True,
            "compensation_available": False,
        },
    }

    def _contract(self, risk="__unset__"):
        c = valid_contract()
        if risk != "__unset__":
            c["risk"] = risk
        return c

    def _codes(self, risk="__unset__"):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_contract(tmp, self._contract(risk))
            errors: list = []
            capability_validation.check_new_contract_risk_metadata(path, errors)
            return [e["code"] for e in errors]

    def test_well_formed_risk_passes(self):
        self.assertEqual(self._codes(self.ELIGIBLE_RISK), [])

    def test_minimal_risk_without_data_flow_passes(self):
        risk = {k: v for k, v in self.ELIGIBLE_RISK.items() if k != "data_flow"}
        self.assertEqual(self._codes(risk), [])

    def test_missing_risk_block_is_rejected(self):
        self.assertIn("contract.missing_risk_metadata", self._codes())

    def test_null_risk_block_is_rejected(self):
        self.assertIn("contract.missing_risk_metadata", self._codes(None))

    def test_unknown_effect_class_is_rejected(self):
        risk = {**self.ELIGIBLE_RISK, "effect_class": "read_only"}
        self.assertIn("contract.invalid_risk_metadata", self._codes(risk))

    def test_unknown_determinism_class_is_rejected(self):
        risk = {**self.ELIGIBLE_RISK, "determinism_class": "random"}
        self.assertIn("contract.invalid_risk_metadata", self._codes(risk))

    def test_missing_reliability_is_rejected(self):
        risk = {k: v for k, v in self.ELIGIBLE_RISK.items() if k != "reliability"}
        self.assertIn("contract.invalid_risk_metadata", self._codes(risk))

    def test_reliability_field_not_boolean_is_rejected(self):
        risk = {
            **self.ELIGIBLE_RISK,
            "reliability": {**self.ELIGIBLE_RISK["reliability"], "retryable": "yes"},
        }
        self.assertIn("contract.invalid_risk_metadata", self._codes(risk))

    def test_allowed_connectors_egress_policy_passes(self):
        risk = {
            **self.ELIGIBLE_RISK,
            "data_flow": {"egress_policy": {"allowed_connectors": ["smtp", "slack"]}},
        }
        self.assertEqual(self._codes(risk), [])

    def test_malformed_egress_policy_is_rejected(self):
        risk = {**self.ELIGIBLE_RISK, "data_flow": {"egress_policy": "allow_all"}}
        self.assertIn("contract.invalid_risk_metadata", self._codes(risk))

    def test_malformed_data_classification_entry_is_rejected(self):
        risk = {
            **self.ELIGIBLE_RISK,
            "data_flow": {
                "egress_policy": "denied",
                "accepted_data_classifications": [{"field_path": "/a", "classification": "secret"}],
            },
        }
        self.assertIn("contract.invalid_risk_metadata", self._codes(risk))


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


class CheckNewContractWasm32ExecutionTests(unittest.TestCase):
    """registry#509: run the newly-published capability's actual wasm32
    artifact under wasmtime, using fixtures derived from the contract."""

    def _write_contract(
        self, tmp: Path, capability_id: str, use_cases=None, properties=None, connector_requirements=None
    ) -> Path:
        path = Path(tmp) / "capabilities" / "example" / capability_id / "1.0.0" / "contract.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        contract = {
            "id": capability_id,
            "use_cases": use_cases or [],
            "inputs": {"schema": {"type": "object", "properties": properties or {}}},
        }
        if connector_requirements is not None:
            contract["connector_requirements"] = connector_requirements
        path.write_text(json.dumps(contract))
        return path

    def _write_crate(self, tmp: Path, crate_name: str) -> None:
        crate_dir = Path(tmp) / "capability-src" / crate_name
        crate_dir.mkdir(parents=True, exist_ok=True)
        (crate_dir / "Cargo.toml").write_text("[package]\nname = \"x\"\n")

    def _build_result(self, wasm_path: Path, returncode=0, stderr=""):
        message = json.dumps(
            {
                "reason": "compiler-artifact",
                "target": {"kind": ["bin"]},
                "filenames": [str(wasm_path)],
            }
        )
        return type(
            "Result", (), {"returncode": returncode, "stdout": message, "stderr": stderr}
        )()

    def _wasmtime_result(self, stdout="{}", returncode=0, stderr=""):
        return type(
            "Result", (), {"returncode": returncode, "stdout": stdout, "stderr": stderr}
        )()

    def test_missing_crate_directory_is_skipped(self):
        # check_new_contract_test_coverage already reports this; this check
        # must not double-report it.
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write_contract(tmp, "example.new-capability")
            errors: list = []
            cwd = os.getcwd()
            try:
                os.chdir(tmp)
                capability_validation.check_new_contract_wasm32_execution(path, errors)
            finally:
                os.chdir(cwd)
            self.assertEqual(errors, [])

    def test_no_happy_use_cases_runs_nothing(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write_contract(tmp, "example.new-capability")
            self._write_crate(tmp, "example-new-capability")
            errors: list = []
            cwd = os.getcwd()
            try:
                os.chdir(tmp)
                with patch("capability_validation.subprocess.run") as mock_run:
                    capability_validation.check_new_contract_wasm32_execution(path, errors)
                mock_run.assert_not_called()
            finally:
                os.chdir(cwd)
            self.assertEqual(errors, [])

    def test_build_failure_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            use_cases = [{"happy": True, "input_example": {"text": "hi"}}]
            path = self._write_contract(tmp, "example.new-capability", use_cases=use_cases)
            self._write_crate(tmp, "example-new-capability")
            errors: list = []
            cwd = os.getcwd()
            try:
                os.chdir(tmp)
                result = type("Result", (), {"returncode": 1, "stdout": "", "stderr": "error"})()
                with patch("capability_validation.subprocess.run", return_value=result):
                    capability_validation.check_new_contract_wasm32_execution(path, errors)
            finally:
                os.chdir(cwd)
            codes = [e["code"] for e in errors]
            self.assertIn("capability.wasm32_build_failed", codes)

    def test_happy_path_execution_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            use_cases = [{"happy": True, "input_example": {"text": "hi"}}]
            path = self._write_contract(tmp, "example.new-capability", use_cases=use_cases)
            self._write_crate(tmp, "example-new-capability")
            wasm_path = Path(tmp) / "out.wasm"
            wasm_path.write_bytes(b"\0asm")
            errors: list = []
            cwd = os.getcwd()
            try:
                os.chdir(tmp)
                build_result = self._build_result(wasm_path)
                run_result = self._wasmtime_result(stdout=json.dumps({"text": "hi"}))
                with patch(
                    "capability_validation.subprocess.run",
                    side_effect=[build_result, run_result],
                ):
                    capability_validation.check_new_contract_wasm32_execution(path, errors)
            finally:
                os.chdir(cwd)
            self.assertEqual(errors, [])

    def test_connector_backed_happy_and_unbound_fixtures(self):
        with tempfile.TemporaryDirectory() as tmp:
            use_cases = [
                {
                    "happy": True,
                    "input_example": {"content_ref": "c1"},
                    "output_example": {
                        "asset_ref": "a1",
                        "result_class": "created",
                    },
                }
            ]
            path = self._write_contract(
                tmp,
                "example.connector-capability",
                use_cases=use_cases,
                connector_requirements=[{"connector_id": "traverse.object-store", "version": "^1"}],
            )
            self._write_crate(tmp, "example-connector-capability")
            wasm_path = Path(tmp) / "out.wasm"
            wasm_path.write_bytes(b"\0asm")
            errors: list = []
            cwd = os.getcwd()
            try:
                os.chdir(tmp)
                build_result = self._build_result(wasm_path)
                happy_run = self._wasmtime_result(
                    stdout=json.dumps({"asset_ref": "a1", "result_class": "created"})
                )
                unbound_run = self._wasmtime_result(
                    stdout=json.dumps(
                        {"asset_ref": "", "result_class": "connector_unavailable"}
                    )
                )
                with patch(
                    "capability_validation.subprocess.run",
                    side_effect=[build_result, happy_run, unbound_run],
                ), patch(
                    "capability_validation._wasm32_fixture_runner_bin",
                    return_value=Path(tmp) / "fake-runner",
                ):
                    (Path(tmp) / "fake-runner").write_text("#!/bin/sh\n")
                    (Path(tmp) / "fake-runner").chmod(0o755)
                    capability_validation.check_new_contract_wasm32_execution(path, errors)
            finally:
                os.chdir(cwd)
            self.assertEqual(errors, [])

    def test_connector_unbound_without_fail_closed_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            use_cases = [
                {
                    "happy": True,
                    "input_example": {"content_ref": "c1"},
                    "output_example": {"asset_ref": "a1", "result_class": "created"},
                }
            ]
            path = self._write_contract(
                tmp,
                "example.connector-capability",
                use_cases=use_cases,
                connector_requirements=[{"connector_id": "traverse.object-store", "version": "^1"}],
            )
            self._write_crate(tmp, "example-connector-capability")
            wasm_path = Path(tmp) / "out.wasm"
            wasm_path.write_bytes(b"\0asm")
            errors: list = []
            cwd = os.getcwd()
            try:
                os.chdir(tmp)
                build_result = self._build_result(wasm_path)
                happy_run = self._wasmtime_result(
                    stdout=json.dumps({"asset_ref": "a1", "result_class": "created"})
                )
                unbound_run = self._wasmtime_result(
                    stdout=json.dumps({"asset_ref": "a1", "result_class": "created"})
                )
                with patch(
                    "capability_validation.subprocess.run",
                    side_effect=[build_result, happy_run, unbound_run],
                ), patch(
                    "capability_validation._wasm32_fixture_runner_bin",
                    return_value=Path(tmp) / "fake-runner",
                ):
                    (Path(tmp) / "fake-runner").write_text("#!/bin/sh\n")
                    (Path(tmp) / "fake-runner").chmod(0o755)
                    capability_validation.check_new_contract_wasm32_execution(path, errors)
            finally:
                os.chdir(cwd)
            codes = [e["code"] for e in errors]
            self.assertIn("capability.connector_unbound_did_not_fail_closed", codes)

    def test_connector_fixture_host_private_keys_are_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            use_cases = [
                {
                    "happy": True,
                    "input_example": {"content_ref": "c1"},
                    "connector_fixture": {
                        "activated": True,
                        "responses": [
                            {
                                "connector_id": "traverse.object-store",
                                "operation": "",
                                "body": {"payload": {"secret": "nope"}},
                            }
                        ],
                    },
                }
            ]
            path = self._write_contract(
                tmp,
                "example.connector-capability",
                use_cases=use_cases,
                connector_requirements=[{"connector_id": "traverse.object-store", "version": "^1"}],
            )
            self._write_crate(tmp, "example-connector-capability")
            wasm_path = Path(tmp) / "out.wasm"
            wasm_path.write_bytes(b"\0asm")
            errors: list = []
            cwd = os.getcwd()
            try:
                os.chdir(tmp)
                build_result = self._build_result(wasm_path)
                with patch(
                    "capability_validation.subprocess.run",
                    return_value=build_result,
                ):
                    capability_validation.check_new_contract_wasm32_execution(path, errors)
            finally:
                os.chdir(cwd)
            codes = [e["code"] for e in errors]
            self.assertIn("capability.connector_fixture_leaks_host_private_data", codes)

    def test_wasm32_panic_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            use_cases = [{"happy": True, "input_example": {"text": "hi"}}]
            path = self._write_contract(tmp, "example.new-capability", use_cases=use_cases)
            self._write_crate(tmp, "example-new-capability")
            wasm_path = Path(tmp) / "out.wasm"
            wasm_path.write_bytes(b"\0asm")
            errors: list = []
            cwd = os.getcwd()
            try:
                os.chdir(tmp)
                build_result = self._build_result(wasm_path)
                run_result = self._wasmtime_result(returncode=1, stderr="panicked")
                with patch(
                    "capability_validation.subprocess.run",
                    side_effect=[build_result, run_result],
                ):
                    capability_validation.check_new_contract_wasm32_execution(path, errors)
            finally:
                os.chdir(cwd)
            codes = [e["code"] for e in errors]
            self.assertIn("capability.wasm32_execution_failed", codes)

    def test_budget_wipe_regression_is_rejected(self):
        # Reproduces registry#487: an oversized-but-schema-legal max_length
        # silently wipes output text that a normal max_length preserved.
        with tempfile.TemporaryDirectory() as tmp:
            use_cases = [{"happy": True, "input_example": {"text": "hello world", "max_length": 6}}]
            properties = {
                "text": {"type": "string"},
                "max_length": {"type": "integer", "minimum": 0},
            }
            path = self._write_contract(
                tmp, "example.new-capability", use_cases=use_cases, properties=properties
            )
            self._write_crate(tmp, "example-new-capability")
            wasm_path = Path(tmp) / "out.wasm"
            wasm_path.write_bytes(b"\0asm")
            errors: list = []
            cwd = os.getcwd()
            try:
                os.chdir(tmp)
                build_result = self._build_result(wasm_path)
                happy_run = self._wasmtime_result(stdout=json.dumps({"text": "hello…", "truncated": True}))
                zero_run = self._wasmtime_result(stdout=json.dumps({"text": "", "truncated": True}))
                oversized_run = self._wasmtime_result(stdout=json.dumps({"text": "", "truncated": True}))
                with patch(
                    "capability_validation.subprocess.run",
                    side_effect=[build_result, happy_run, zero_run, oversized_run],
                ):
                    capability_validation.check_new_contract_wasm32_execution(path, errors)
            finally:
                os.chdir(cwd)
            codes = [e["code"] for e in errors]
            self.assertIn("capability.wasm32_budget_wipe_regression", codes)

    def test_well_behaved_oversized_budget_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            use_cases = [{"happy": True, "input_example": {"text": "hello world", "max_length": 6}}]
            properties = {
                "text": {"type": "string"},
                "max_length": {"type": "integer", "minimum": 0},
            }
            path = self._write_contract(
                tmp, "example.new-capability", use_cases=use_cases, properties=properties
            )
            self._write_crate(tmp, "example-new-capability")
            wasm_path = Path(tmp) / "out.wasm"
            wasm_path.write_bytes(b"\0asm")
            errors: list = []
            cwd = os.getcwd()
            try:
                os.chdir(tmp)
                build_result = self._build_result(wasm_path)
                happy_run = self._wasmtime_result(stdout=json.dumps({"text": "hello…", "truncated": True}))
                zero_run = self._wasmtime_result(stdout=json.dumps({"text": "", "truncated": True}))
                oversized_run = self._wasmtime_result(
                    stdout=json.dumps({"text": "hello world", "truncated": False})
                )
                with patch(
                    "capability_validation.subprocess.run",
                    side_effect=[build_result, happy_run, zero_run, oversized_run],
                ):
                    capability_validation.check_new_contract_wasm32_execution(path, errors)
            finally:
                os.chdir(cwd)
            self.assertEqual(errors, [])


class ModelBackedWasm32ExecutionTests(unittest.TestCase):
    """registry#609: ai.model_backed contracts fetch and sha256-verify their
    declared weight blobs, then build with --features full-model."""

    BLOB = b"weights"
    BLOB_SHA = hashlib.sha256(b"weights").hexdigest()
    URL = "https://github.com/traverse-framework/registry/releases/download/artifacts/example.agent-1.0.0/w.bin"

    def _setup(self, tmp, manifest=None, model_backed=True, write_blob=False):
        path = Path(tmp) / "capabilities" / "example" / "example.agent" / "1.0.0" / "contract.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        contract = {
            "id": "example.agent",
            "use_cases": [{"happy": True, "input_example": {"text": "hi"}}],
            "inputs": {"schema": {"type": "object", "properties": {}}},
        }
        if model_backed:
            contract["ai"] = {"model_backed": True, "models": ["example/model"]}
        path.write_text(json.dumps(contract))
        crate_dir = Path(tmp) / "capability-src" / "example-agent"
        crate_dir.mkdir(parents=True, exist_ok=True)
        (crate_dir / "Cargo.toml").write_text("[package]\nname = \"x\"\n")
        if manifest is not None:
            (crate_dir / "model-weights.json").write_text(
                manifest if isinstance(manifest, str) else json.dumps(manifest)
            )
        if write_blob:
            (crate_dir / "data").mkdir()
            (crate_dir / "data" / "w.bin").write_bytes(self.BLOB)
        wasm_path = Path(tmp) / "out.wasm"
        wasm_path.write_bytes(b"\0asm")
        return path, crate_dir, wasm_path

    def _manifest(self, path="data/w.bin", url=None, sha256=None):
        return {"files": [{"path": path, "url": url or self.URL, "sha256": sha256 or self.BLOB_SHA}]}

    def _results(self, wasm_path):
        build = type("R", (), {
            "returncode": 0,
            "stdout": json.dumps({"reason": "compiler-artifact", "target": {"kind": ["bin"]}, "filenames": [str(wasm_path)]}),
            "stderr": "",
        })()
        run = type("R", (), {"returncode": 0, "stdout": json.dumps({"text": "hi"}), "stderr": ""})()
        return [build, run]

    def _check(self, tmp, path, run_side_effect=None, urlopen=None):
        errors: list = []
        cwd = os.getcwd()
        try:
            os.chdir(tmp)
            with patch("capability_validation.subprocess.run", side_effect=run_side_effect or []) as run:
                if urlopen is not None:
                    with patch("capability_validation.urllib.request.urlopen", urlopen):
                        capability_validation.check_new_contract_wasm32_execution(path, errors)
                else:
                    capability_validation.check_new_contract_wasm32_execution(path, errors)
        finally:
            os.chdir(cwd)
        return errors, run

    def _codes(self, errors):
        return [e["code"] for e in errors]

    def _fake_urlopen(self, body=None, exc=None):
        import io

        class _Response(io.BytesIO):
            def __enter__(self):
                return self

            def __exit__(self, *args):
                self.close()

        def _open(request, timeout=None):
            if exc is not None:
                raise exc
            return _Response(body if body is not None else self.BLOB)

        return _open

    def test_missing_manifest_fails_without_building(self):
        with tempfile.TemporaryDirectory() as tmp:
            path, _, _ = self._setup(tmp)
            errors, run = self._check(tmp, path)
            self.assertEqual(self._codes(errors), ["capability.model_weights_manifest_missing"])
            run.assert_not_called()

    def test_unparseable_manifest_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            path, _, _ = self._setup(tmp, manifest="{not json")
            errors, run = self._check(tmp, path)
            self.assertEqual(self._codes(errors), ["capability.model_weights_manifest_invalid"])
            run.assert_not_called()

    def test_empty_files_array_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            path, _, _ = self._setup(tmp, manifest={"files": []})
            errors, _ = self._check(tmp, path)
            self.assertEqual(self._codes(errors), ["capability.model_weights_manifest_invalid"])

    def test_malformed_entries_fail_closed(self):
        cases = [
            self._manifest(path="../../../etc/passwd"),
            {"files": [{"path": "", "url": self.URL, "sha256": self.BLOB_SHA}]},
            {"files": ["data/w.bin"]},
            self._manifest(url="https://huggingface.co/x/resolve/main/w.bin"),
            self._manifest(sha256="ABC"),
        ]
        for manifest in cases:
            with self.subTest(manifest=manifest), tempfile.TemporaryDirectory() as tmp:
                path, _, _ = self._setup(tmp, manifest=manifest)
                errors, run = self._check(tmp, path)
                self.assertEqual(self._codes(errors), ["capability.model_weights_manifest_invalid"])
                run.assert_not_called()

    def test_present_blob_builds_full_model_with_long_timeout(self):
        with tempfile.TemporaryDirectory() as tmp:
            path, _, wasm_path = self._setup(tmp, manifest=self._manifest(), write_blob=True)
            errors, run = self._check(tmp, path, run_side_effect=self._results(wasm_path))
            self.assertEqual(errors, [])
            build_args = run.call_args_list[0].args[0]
            self.assertIn("full-model", build_args)
            self.assertEqual(build_args[build_args.index("full-model") - 1], "--features")
            self.assertEqual(
                run.call_args_list[1].kwargs["timeout"],
                capability_validation.MODEL_BACKED_FIXTURE_TIMEOUT_SECONDS,
            )

    def test_missing_blob_is_fetched_and_verified(self):
        with tempfile.TemporaryDirectory() as tmp:
            path, crate_dir, wasm_path = self._setup(tmp, manifest=self._manifest())
            errors, _ = self._check(
                tmp, path, run_side_effect=self._results(wasm_path), urlopen=self._fake_urlopen()
            )
            self.assertEqual(errors, [])
            self.assertEqual((crate_dir / "data" / "w.bin").read_bytes(), self.BLOB)
            self.assertFalse((crate_dir / "data" / "w.bin.partial").exists())

    def test_fetched_blob_digest_mismatch_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            path, crate_dir, _ = self._setup(tmp, manifest=self._manifest())
            errors, run = self._check(tmp, path, urlopen=self._fake_urlopen(body=b"tampered"))
            self.assertEqual(self._codes(errors), ["capability.model_weights_digest_mismatch"])
            self.assertFalse((crate_dir / "data" / "w.bin").exists())
            self.assertFalse((crate_dir / "data" / "w.bin.partial").exists())
            run.assert_not_called()

    def test_unreachable_blob_fails(self):
        import urllib.error

        with tempfile.TemporaryDirectory() as tmp:
            path, _, _ = self._setup(tmp, manifest=self._manifest())
            errors, run = self._check(
                tmp, path, urlopen=self._fake_urlopen(exc=urllib.error.URLError("404"))
            )
            self.assertEqual(self._codes(errors), ["capability.model_weights_unavailable"])
            run.assert_not_called()

    def test_non_agent_build_is_unchanged(self):
        with tempfile.TemporaryDirectory() as tmp:
            path, _, wasm_path = self._setup(tmp, model_backed=False)
            errors, run = self._check(tmp, path, run_side_effect=self._results(wasm_path))
            self.assertEqual(errors, [])
            self.assertNotIn("--features", run.call_args_list[0].args[0])
            self.assertEqual(run.call_args_list[1].kwargs["timeout"], 30)


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

    def _run(self, tmp, pr_added_dirs=None):
        errors: list = []
        cwd = os.getcwd()
        try:
            os.chdir(tmp)
            capability_validation.check_signature_siblings(errors, pr_added_dirs)
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

    def test_unsigned_version_added_by_this_pr_is_exempt_when_enforced(self):
        """spec 007 Amendment FR-007/FR-009: the first publish of an
        artifact-bearing capability cannot carry its own signature.json (the
        sign-artifacts job runs post-merge), so an ADDED-in-PR version dir must
        not hard-fail even with the marker committed."""
        with tempfile.TemporaryDirectory() as tmp:
            self._tree(tmp, signed=False, enforced=True)
            added = {"capabilities/core/core.example/1.0.0"}
            self.assertEqual(self._run(tmp, pr_added_dirs=added), [])

    def test_preexisting_unsigned_version_still_fails_even_with_a_pr_add(self):
        """The drift / self-healing net must not be silenced just because the
        same PR also adds a (different) new capability."""
        with tempfile.TemporaryDirectory() as tmp:
            self._tree(tmp, signed=False, enforced=True)
            unrelated_add = {"capabilities/core/core.other/2.0.0"}
            self.assertIn(
                "signature.missing",
                [e["code"] for e in self._run(tmp, pr_added_dirs=unrelated_add)],
            )


class LicensingBackfillImmutabilityExceptionTests(unittest.TestCase):
    """decision-log 119 / 123: pure top-level `licensing` addition is
    allowed when previously absent; any other edit still fails."""

    SAMPLE_PATH = (
        "capabilities/text/text.detect-entities/1.0.0/contract.json"
    )

    def test_licensing_only_addition_helper_accepts_pure_add(self):
        before = {"id": "x", "version": "1.0.0"}
        after = {
            "id": "x",
            "version": "1.0.0",
            "licensing": {"spdx_expression": "Apache-2.0"},
        }

        def fake_show(cmd, text=True):
            ref = cmd[2]
            if ref.startswith("base:"):
                return json.dumps(before)
            return json.dumps(after)

        with patch("subprocess.check_output", side_effect=fake_show):
            self.assertTrue(
                capability_validation._licensing_only_addition(
                    "base", "head", self.SAMPLE_PATH
                )
            )

    def test_licensing_only_addition_rejects_other_edits(self):
        before = {"id": "x", "version": "1.0.0"}
        after = {
            "id": "x",
            "version": "1.0.1",
            "licensing": {"spdx_expression": "Apache-2.0"},
        }

        def fake_show(cmd, text=True):
            ref = cmd[2]
            if ref.startswith("base:"):
                return json.dumps(before)
            return json.dumps(after)

        with patch("subprocess.check_output", side_effect=fake_show):
            self.assertFalse(
                capability_validation._licensing_only_addition(
                    "base", "head", self.SAMPLE_PATH
                )
            )

    def test_check_immutability_allows_licensing_only_edit_any_path(self):
        path = "capabilities/approval/approval.decision-apply/1.0.0/contract.json"

        def fake_check_output(cmd, text=True):
            if cmd[1] == "diff":
                return f"M\t{path}\n"
            ref = cmd[2]
            sha, _, _rest = ref.partition(":")
            before = {"id": "approval.decision-apply", "version": "1.0.0"}
            after = {
                **before,
                "licensing": {
                    "spdx_expression": "Apache-2.0",
                    "commercial_use": "allowed",
                    "redistribution": "allowed",
                    "attribution_required": True,
                    "verification": {"status": "maintainer-declared"},
                },
            }
            if sha == "BASE":
                return json.dumps(before)
            return json.dumps(after)

        errors: list = []
        with patch("subprocess.check_output", side_effect=fake_check_output):
            capability_validation.check_immutability("BASE", "HEAD", errors)
        self.assertEqual(errors, [])

    def test_check_immutability_still_rejects_non_licensing_edit(self):
        other = "capabilities/core/core.authorize/1.0.0/contract.json"

        def fake_check_output(cmd, text=True):
            if cmd[1] == "diff":
                return f"M\t{other}\n"
            ref = cmd[2]
            sha, _, _rest = ref.partition(":")
            before = {"id": "core.authorize", "version": "1.0.0", "summary": "a"}
            after = {"id": "core.authorize", "version": "1.0.0", "summary": "b"}
            if sha == "BASE":
                return json.dumps(before)
            return json.dumps(after)

        errors: list = []
        with patch("subprocess.check_output", side_effect=fake_check_output):
            capability_validation.check_immutability("BASE", "HEAD", errors)
        self.assertEqual(
            [e["code"] for e in errors],
            ["capabilities.contract_modified"],
        )

    def test_changed_contract_gates_skip_licensing_only_modification(self):
        path = "capabilities/core/core.aggregate-team-action-health/1.0.0/contract.json"
        before = {"id": "core.aggregate-team-action-health", "version": "1.0.0"}
        after = {
            **before,
            "licensing": {"spdx_expression": "Apache-2.0"},
        }

        def fake_check_output(cmd, text=True):
            if cmd[1] == "diff":
                return f"M\t{path}\n"
            ref = cmd[2]
            sha, _, _rest = ref.partition(":")
            if sha == "BASE":
                return json.dumps(before)
            return json.dumps(after)

        errors: list = []
        with patch("subprocess.check_output", side_effect=fake_check_output):
            capability_validation.check_new_contracts_declare_risk_metadata(
                "BASE", "HEAD", errors
            )
            capability_validation.check_new_contracts_declare_authoring_method(
                "BASE", "HEAD", errors
            )
            capability_validation.check_new_contracts_use_cases_surface_coverage(
                "BASE", "HEAD", errors
            )
        self.assertEqual(errors, [])



if __name__ == "__main__":
    unittest.main()
