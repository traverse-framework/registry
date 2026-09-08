#!/usr/bin/env python3
"""Unit tests for observed-lineage join in gather_catalog_data (registry#256)."""

import importlib.util
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

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

    def test_legacy_ids_still_resolve_through_the_explicit_dict(self):
        # validate-luhn's crate is NOT its canonical name (validation-validate-luhn).
        self.assertEqual(self.mod.CURRENT_CRATE_FOR_ID["validation.validate-luhn"], "validate-luhn")
        self.assertEqual(self.mod.resolve_current_crate("validation.validate-luhn"), "validate-luhn")

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


if __name__ == "__main__":
    unittest.main()
