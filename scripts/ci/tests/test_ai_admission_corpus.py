#!/usr/bin/env python3
"""Proves scripts/ci/capability_validation.py agrees with every fixture in
scripts/ci/fixtures/ai_admission_corpus.json (decision-log entry 129,
registry#627).

The corpus is the cross-repo parity contract for the `ai` admission rules
(spec 001 FR-017, spec 026). traverse-cli `capability publish` vendors a
pinned copy and must reach the same verdict on every `contract_decidable`
fixture (traverse Spec 056 v1.1.0 FR-019). A rule change here without a
matching fixture change fails this test.

Run with: python3 -m unittest scripts/ci/tests/test_ai_admission_corpus.py
"""

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

MODULE_PATH = Path(__file__).resolve().parents[1] / "capability_validation.py"
CORPUS_PATH = Path(__file__).resolve().parents[1] / "fixtures" / "ai_admission_corpus.json"
SPDX_SYMBOLS_PATH = Path(__file__).resolve().parents[1] / "fixtures" / "spdx_symbols.json"
spec = importlib.util.spec_from_file_location("capability_validation", MODULE_PATH)
capability_validation = importlib.util.module_from_spec(spec)
sys.modules["capability_validation"] = capability_validation
spec.loader.exec_module(capability_validation)

CAPABILITY_ID = "example.detect-things"
TIERS = {"contract_decidable", "ci_only"}
EVALUATED_AS = {"newly_added", "existing"}


def load_corpus():
    return json.loads(CORPUS_PATH.read_text())


def _declared_converted_digests(ai) -> list:
    models = ai.get("models") if isinstance(ai, dict) else None
    digests = []
    for model_ref in models if isinstance(models, list) else []:
        derivation = model_ref.get("derivation") if isinstance(model_ref, dict) else None
        if isinstance(derivation, dict) and isinstance(derivation.get("converted_sha256"), str):
            digests.append(derivation["converted_sha256"])
    return digests


def evaluate_fixture(fixture: dict) -> list:
    """Sorted, de-duplicated error codes CI produces for `fixture`.

    `existing` runs only the whole-tree `ai` check. `newly_added` also runs
    the new-contract forward gates (object shape, spec 026 rights).
    `contract_decidable` fixtures assume every ci_only check passes: no
    network fetch, and every declared `derivation.converted_sha256` is listed
    in model-weights.json. `ci_only` fixtures take both from `ci_context`.
    Cross-contract rights drift needs other published contracts and is not
    modelled here."""
    contract = {"id": CAPABILITY_ID, "namespace": "example", "owner": {"team": "fixtures"}, "version": "1.0.0"}
    if "ai" in fixture:
        contract["ai"] = fixture["ai"]
    context = fixture.get("ci_context") or {}
    errors = []
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        path = root / "capabilities" / "example" / CAPABILITY_ID / "1.0.0" / "contract.json"
        path.parent.mkdir(parents=True)
        path.write_text(json.dumps(contract))
        capability_validation.validate_ai_declaration(path, contract, errors)
        if fixture["evaluated_as"] == "newly_added":
            capability_validation.check_new_contract_ai_models_object_shape(path, errors)
            if fixture["tier"] == "ci_only":
                digests = set(context.get("model_weight_digests", []))
                fetched = context.get("fetched_sha256", {})

                def fake_fetch(url):
                    value = fetched.get(url)
                    if value is None:
                        raise OSError(f"fixture: {url} unreachable")
                    return value

                with patch.object(capability_validation, "_model_weight_digests", return_value=digests), patch.object(
                    capability_validation, "_fetch_sha256_hex", side_effect=fake_fetch
                ):
                    capability_validation.check_new_contract_model_rights(path, errors, root=root, fetch=True)
            else:
                digests = set(_declared_converted_digests(fixture.get("ai")))
                with patch.object(capability_validation, "_model_weight_digests", return_value=digests):
                    capability_validation.check_new_contract_model_rights(path, errors, root=root, fetch=False)
    return sorted({error["code"] for error in errors})


class AiAdmissionCorpusTest(unittest.TestCase):
    def setUp(self):
        if capability_validation.get_spdx_licensing is None:
            self.skipTest("license-expression is required (CI installs the pinned version)")
        self.corpus = load_corpus()

    def test_manifest_shape(self):
        self.assertRegex(self.corpus["corpus_version"], capability_validation.SEMVER_RE)
        self.assertEqual(self.corpus["license_expression_version"], capability_validation.LICENSE_EXPRESSION_PIN)
        names = [fixture["name"] for fixture in self.corpus["fixtures"]]
        self.assertEqual(len(names), len(set(names)), "fixture names must be unique")
        ci_only_codes = set(self.corpus["ci_only_codes"])
        for fixture in self.corpus["fixtures"]:
            with self.subTest(fixture=fixture["name"]):
                self.assertIn(fixture["tier"], TIERS)
                self.assertIn(fixture["evaluated_as"], EVALUATED_AS)
                self.assertIn(fixture["expect"], {"accept", "reject"})
                self.assertEqual(fixture["expect"] == "accept", not fixture["expect_codes"])
                self.assertEqual(fixture["expect_codes"], sorted(set(fixture["expect_codes"])))
                if fixture["tier"] == "contract_decidable":
                    self.assertFalse(ci_only_codes & set(fixture["expect_codes"]))
                    self.assertNotIn("ci_context", fixture)
                else:
                    self.assertIn("ci_context", fixture)

    def test_spdx_symbol_export_is_current(self):
        """The exported table must equal the pinned license-expression's
        symbols. Regenerate after bumping LICENSE_EXPRESSION_PIN with:
        python3 -c "import json,sys; sys.path.insert(0,'scripts/ci'); import capability_validation as cv;
        open('scripts/ci/fixtures/spdx_symbols.json','w').write(json.dumps(cv.spdx_symbol_table(), indent=0, ensure_ascii=False) + '\\n')"
        """
        self.assertEqual(json.loads(SPDX_SYMBOLS_PATH.read_text()), capability_validation.spdx_symbol_table())
        self.assertEqual(self.corpus["spdx_symbols"], "scripts/ci/fixtures/spdx_symbols.json")

    def test_validator_agrees_with_every_fixture(self):
        for fixture in self.corpus["fixtures"]:
            with self.subTest(fixture=fixture["name"]):
                self.assertEqual(evaluate_fixture(fixture), fixture["expect_codes"])


if __name__ == "__main__":
    unittest.main()
