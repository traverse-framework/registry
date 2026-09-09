#!/usr/bin/env python3
"""Unit tests for observed-lineage join in gather_catalog_data (registry#256)."""

import importlib.util
import json
import os
import sys
import tempfile
import types
import unittest
from pathlib import Path
from unittest import mock

REPO_ROOT = Path(__file__).resolve().parents[3]
MODULE_PATH = REPO_ROOT / "scripts" / "ci" / "gather_catalog_data.py"
os.chdir(REPO_ROOT)


def load_module():
    spec = importlib.util.spec_from_file_location("gather_catalog_data", MODULE_PATH)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


class ResolveCurrentCrateTests(unittest.TestCase):
    """decision-log entry 95: a capability published after specs/018 must get
    its coverage badge via the canonical id-with-dots-as-dashes crate name,
    with no hand-added CURRENT_CRATE_FOR_ID entry."""

    def setUp(self):
        self.mod = load_module()

    def test_canonical_rule_replaces_every_dot(self):
        self.assertEqual(
            self.mod.canonical_crate_for_id("commerce.cart-line-changed"),
            "commerce-cart-line-changed",
        )
        self.assertEqual(
            self.mod.canonical_crate_for_id("core.action-item.status"),
            "core-action-item-status",
        )

    def test_ids_still_resolve_through_the_explicit_dict(self):
        # Explicit CURRENT_CRATE_FOR_ID entries are consulted first.
        self.assertEqual(
            self.mod.CURRENT_CRATE_FOR_ID["approval.decision-apply"], "approval-decision-apply"
        )
        self.assertEqual(
            self.mod.resolve_current_crate("approval.decision-apply"), "approval-decision-apply"
        )

    def test_renamed_validation_and_formatting_crates_resolve_via_canonical(self):
        # registry#420: the 5 crates that used to be legacy-named
        # (validate-luhn, ..., format-currency) were renamed to their
        # canonical <id-with-dashes> dirs and their CURRENT_CRATE_FOR_ID
        # entries dropped -- they now resolve by the canonical rule.
        for cid, crate in (
            ("validation.validate-luhn", "validation-validate-luhn"),
            ("validation.validate-email", "validation-validate-email"),
            ("validation.normalize-phone-number", "validation-normalize-phone-number"),
            ("validation.score-password-strength", "validation-score-password-strength"),
            ("formatting.format-currency", "formatting-format-currency"),
        ):
            self.assertNotIn(cid, self.mod.CURRENT_CRATE_FOR_ID)
            self.assertEqual(self.mod.resolve_current_crate(cid), crate)

    def test_post_spec018_id_resolves_via_canonical_fallback(self):
        # This id has real source at capability-src/commerce-cart-line-changed/
        # but no CURRENT_CRATE_FOR_ID entry -- it used to render with no badge.
        self.assertNotIn("commerce.cart-line-changed", self.mod.CURRENT_CRATE_FOR_ID)
        self.assertEqual(
            self.mod.resolve_current_crate("commerce.cart-line-changed"),
            "commerce-cart-line-changed",
        )

    def test_unknown_id_resolves_to_none(self):
        self.assertIsNone(self.mod.resolve_current_crate("nope.does-not-exist"))

    def test_every_current_capability_now_resolves(self):
        """The regression that lost 14 badges: iterate every latest
        non-deprecated contract and require a resolvable crate."""
        latest: dict = {}
        for contract_path in sorted(Path("capabilities").rglob("contract.json")):
            contract = json.loads(contract_path.read_text())
            cid, ver = contract["id"], contract["version"]
            deprecated = (contract_path.parent / "deprecated.json").is_file()
            cur = latest.get(cid)
            if cur is None or self.mod.semver_tuple(ver) > self.mod.semver_tuple(cur[0]):
                latest[cid] = (ver, deprecated)
        unresolved = sorted(
            cid for cid, (ver, dep) in latest.items()
            if not dep and self.mod.resolve_current_crate(cid) is None
        )
        self.assertEqual(unresolved, [], f"capabilities with no resolvable crate: {unresolved}")


class AttachObservedLineageTests(unittest.TestCase):
    def setUp(self):
        self.mod = load_module()

    def test_fixture_produces_declared_match_and_drift(self):
        entries = [
            {
                "deprecated": False,
                "product": {
                    "contract": {
                        "id": "core.action-item.status-transitioned",
                        "version": "1.0.0",
                        "publishers": [
                            {
                                "capability_id": "core.transition-action-status",
                                "version": "1.1.0",
                            }
                        ],
                        "subscribers": [],
                    }
                },
            }
        ]
        # Run against the real repo fixture when available.
        self.mod.attach_observed_lineage(entries)
        lineage = entries[0]["observed_lineage"]
        self.assertEqual(len(lineage["interactions"]), 2)
        roles = {item["role"] for item in lineage["interactions"]}
        self.assertEqual(roles, {"publisher", "subscriber"})
        self.assertEqual(len(lineage["drift"]), 1)
        self.assertEqual(lineage["drift"][0]["kind"], "undeclared_subscriber")
        self.assertEqual(
            lineage["drift"][0]["capability_id"], "core.unexpected-status-watcher"
        )
        # Declared publisher observation must not produce drift.
        drift_caps = {item["capability_id"] for item in lineage["drift"]}
        self.assertNotIn("core.transition-action-status", drift_caps)

    def test_missing_fixture_yields_empty_lineage(self):
        entries = [
            {
                "deprecated": False,
                "product": {
                    "contract": {
                        "id": "core.action-item.status-transitioned",
                        "version": "1.0.0",
                        "publishers": [],
                        "subscribers": [],
                    }
                },
            }
        ]
        original = self.mod.OBSERVED_LINEAGE_FIXTURE
        try:
            self.mod.OBSERVED_LINEAGE_FIXTURE = Path(
                tempfile.mkdtemp()
            ) / "does-not-exist.json"
            self.mod.attach_observed_lineage(entries)
        finally:
            self.mod.OBSERVED_LINEAGE_FIXTURE = original
        self.assertEqual(
            entries[0]["observed_lineage"],
            {"interactions": [], "drift": []},
        )


class ResolveCapabilityRiskTests(unittest.TestCase):
    """spec 024-capability-risk-classification-adoption FR-003/FR-004."""

    def setUp(self):
        self.mod = load_module()

    def _fake_run(self, stdout, returncode=0):
        def run(cmd, capture_output, text, check):  # noqa: ARG001
            return types.SimpleNamespace(returncode=returncode, stdout=stdout, stderr="boom")
        return run

    def test_parses_binary_output_into_reference_map(self):
        payload = json.dumps({
            "capabilities": [
                {
                    "reference": "core/core.thing@1.0.0",
                    "risk": {"effect_class": "pure_read"},
                    "is_automatic_eligible": True,
                    "risk_source": "declared",
                }
            ]
        })
        with mock.patch.object(self.mod.subprocess, "run", self._fake_run(payload)):
            out = self.mod.resolve_capability_risk()
        self.assertEqual(
            out["core/core.thing@1.0.0"],
            {"risk": {"effect_class": "pure_read"}, "is_automatic_eligible": True, "risk_source": "declared"},
        )

    def test_nonzero_exit_is_fatal(self):
        with mock.patch.object(self.mod.subprocess, "run", self._fake_run("", returncode=1)):
            with self.assertRaises(RuntimeError):
                self.mod.resolve_capability_risk()

    def test_env_bin_is_used_when_set(self):
        seen = {}

        def run(cmd, capture_output, text, check):  # noqa: ARG001
            seen["cmd"] = cmd
            return types.SimpleNamespace(returncode=0, stdout='{"capabilities": []}', stderr="")

        with mock.patch.dict(os.environ, {"RESOLVE_CAPABILITY_RISK_BIN": "/opt/rcr"}), \
             mock.patch.object(self.mod.subprocess, "run", run):
            self.mod.resolve_capability_risk()
        self.assertEqual(seen["cmd"][:1], ["/opt/rcr"])


if __name__ == "__main__":
    unittest.main()
