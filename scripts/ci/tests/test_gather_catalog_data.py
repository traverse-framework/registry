#!/usr/bin/env python3
"""Unit tests for observed-lineage join in gather_catalog_data (registry#256)."""

import importlib.util
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
