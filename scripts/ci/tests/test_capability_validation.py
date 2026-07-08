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


if __name__ == "__main__":
    unittest.main()
