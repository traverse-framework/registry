#!/usr/bin/env python3
"""Tests for scripts/metrics/publication_lifecycle_export.py pure helpers
(specs/022-publication-lifecycle-measurement FR-014, registry#354).

Run with: python3 -m unittest scripts/metrics/tests/test_publication_lifecycle_export.py
"""

import importlib.util
import sys
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).resolve().parents[1] / "publication_lifecycle_export.py"
spec = importlib.util.spec_from_file_location("publication_lifecycle_export", MODULE_PATH)
ple = importlib.util.module_from_spec(spec)
sys.modules["publication_lifecycle_export"] = ple
spec.loader.exec_module(ple)


PURE_CONTRACT = {
    "dependencies": [],
    "connector_requirements": [],
    "emits": [],
    "consumes": [],
    "side_effects": [{"kind": "memory_only"}],
    "execution": {
        "constraints": {
            "host_api_access": "none",
            "filesystem_access": "none",
            "network_access": "forbidden",
        }
    },
    "inputs": {
        "schema": {
            "properties": {
                "a": {"type": "string"},
                "b": {
                    "type": "object",
                    "properties": {"c": {"type": "integer"}, "d": {"type": "integer"}},
                },
            }
        }
    },
    "outputs": {"schema": {"properties": {"x": {"type": "string"}}}},
}


class SchemaFieldCountTests(unittest.TestCase):
    def test_counts_recursively_across_inputs_and_outputs(self):
        # inputs: a, b (2) + c, d (2) = 4 ; outputs: x (1) => 5
        self.assertEqual(ple.compute_schema_field_count(PURE_CONTRACT), 5)

    def test_missing_schemas_count_zero(self):
        self.assertEqual(ple.compute_schema_field_count({}), 0)


class EffectfulTests(unittest.TestCase):
    def test_pure_contract_is_not_effectful(self):
        self.assertFalse(ple.compute_effectful(PURE_CONTRACT))

    def test_non_memory_side_effect_is_effectful(self):
        c = dict(PURE_CONTRACT, side_effects=[{"kind": "network_call"}])
        self.assertTrue(ple.compute_effectful(c))

    def test_network_access_allowed_is_effectful(self):
        c = dict(
            PURE_CONTRACT,
            execution={"constraints": {"network_access": "allowed"}},
        )
        self.assertTrue(ple.compute_effectful(c))

    def test_host_api_access_is_effectful(self):
        c = dict(
            PURE_CONTRACT,
            execution={"constraints": {"host_api_access": "model", "network_access": "forbidden"}},
        )
        self.assertTrue(ple.compute_effectful(c))

    def test_non_empty_emits_is_effectful(self):
        c = dict(PURE_CONTRACT, emits=[{"id": "core.thing.happened@1.0.0"}])
        self.assertTrue(ple.compute_effectful(c))


class BuildRubricTests(unittest.TestCase):
    def test_pure_no_spec_no_persona_is_simple(self):
        rubric = ple.build_rubric(PURE_CONTRACT, ["capabilities/core/core.x/1.0.0/contract.json"], 20)
        self.assertEqual(ple.rubric_verdict(rubric), "simple")
        self.assertEqual(rubric["schema_field_count"], 5)
        self.assertFalse(rubric["changed_specs"])
        self.assertFalse(rubric["added_persona"])

    def test_changed_spec_makes_it_complex(self):
        rubric = ple.build_rubric(
            PURE_CONTRACT,
            ["capabilities/core/core.x/1.0.0/contract.json", "specs/001-registry-foundation/spec.md"],
            20,
        )
        self.assertTrue(rubric["changed_specs"])
        self.assertEqual(ple.rubric_verdict(rubric), "complex")

    def test_added_persona_makes_it_complex(self):
        rubric = ple.build_rubric(
            PURE_CONTRACT,
            ["capabilities/core/core.x/1.0.0/contract.json", "personas/new-persona/1.0.0/persona.json"],
            20,
        )
        self.assertTrue(rubric["added_persona"])
        self.assertEqual(ple.rubric_verdict(rubric), "complex")

    def test_dependency_makes_it_complex(self):
        c = dict(PURE_CONTRACT, dependencies=[{"id": "core.other", "version_range": "^1"}])
        rubric = ple.build_rubric(c, [], 20)
        self.assertEqual(rubric["dependency_count"], 1)
        self.assertEqual(ple.rubric_verdict(rubric), "complex")

    def test_large_schema_makes_it_complex(self):
        rubric = ple.build_rubric(PURE_CONTRACT, [], 3)  # 5 > 3
        self.assertEqual(ple.rubric_verdict(rubric), "complex")


class TimeHelperTests(unittest.TestCase):
    def test_derive_period_is_month_only(self):
        self.assertEqual(ple.derive_period("2026-09-06T03:44:16Z"), "2026-09")

    def test_hours_between_floors(self):
        self.assertEqual(
            ple.hours_between("2026-09-06T00:00:00Z", "2026-09-06T06:30:00Z"), 6
        )

    def test_hours_between_never_negative(self):
        self.assertEqual(
            ple.hours_between("2026-09-06T06:00:00Z", "2026-09-06T05:00:00Z"), 0
        )

    def test_count_review_rounds_ignores_pending(self):
        reviews = [
            {"state": "COMMENTED"},
            {"state": "CHANGES_REQUESTED"},
            {"state": "PENDING"},
            {"state": "APPROVED"},
        ]
        self.assertEqual(ple.count_review_rounds(reviews), 3)


class BuildRecordTests(unittest.TestCase):
    BASE_META = {
        "number": 358,
        "createdAt": "2026-09-06T00:00:00Z",
        "mergedAt": "2026-09-06T06:00:00Z",
        "closedAt": "2026-09-06T06:00:00Z",
        "reviews": [{"state": "APPROVED"}],
    }

    def test_merged_pure_pr_builds_simple_record(self):
        record = ple.build_record(
            358, self.BASE_META, PURE_CONTRACT,
            ["capabilities/core/core.x/1.0.0/contract.json"], 20, None, None,
        )
        self.assertEqual(record["outcome"], "merged")
        self.assertEqual(record["period"], "2026-09")
        self.assertEqual(record["hours_open_to_conclusion"], 6)
        self.assertEqual(record["review_rounds"], 1)
        self.assertEqual(record["complexity"]["computed"], "simple")
        self.assertEqual(record["complexity"]["final"], "simple")
        self.assertFalse(record["complexity"]["override"])
        self.assertIsNone(record["complexity"]["override_reason"])
        self.assertEqual(record["ci_failure_counts"], {})

    def test_override_records_reason_and_flips_final(self):
        record = ple.build_record(
            358, self.BASE_META, PURE_CONTRACT,
            ["capabilities/core/core.x/1.0.0/contract.json"], 20,
            "complex", "small schema but the reconciliation logic is subtle",
        )
        self.assertEqual(record["complexity"]["computed"], "simple")
        self.assertEqual(record["complexity"]["final"], "complex")
        self.assertTrue(record["complexity"]["override"])
        self.assertIn("subtle", record["complexity"]["override_reason"])

    def test_override_matching_computed_is_rejected(self):
        with self.assertRaises(SystemExit):
            ple.build_record(
                358, self.BASE_META, PURE_CONTRACT,
                ["capabilities/core/core.x/1.0.0/contract.json"], 20, "simple", "x",
            )

    def test_override_without_reason_is_rejected(self):
        with self.assertRaises(SystemExit):
            ple.build_record(
                358, self.BASE_META, PURE_CONTRACT,
                ["capabilities/core/core.x/1.0.0/contract.json"], 20, "complex", "  ",
            )

    def test_open_pr_is_rejected(self):
        meta = dict(self.BASE_META, mergedAt=None, closedAt=None)
        with self.assertRaises(SystemExit):
            ple.build_record(358, meta, PURE_CONTRACT, [], 20, None, None)


if __name__ == "__main__":
    unittest.main()
