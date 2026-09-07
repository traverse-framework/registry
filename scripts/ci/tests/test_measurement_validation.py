#!/usr/bin/env python3
"""Tests for scripts/ci/measurement_validation.py
(specs/022-publication-lifecycle-measurement, registry#354, decision-log entry 83).

Run with: python3 -m unittest scripts/ci/tests/test_measurement_validation.py
"""

import contextlib
import importlib.util
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

MODULE_PATH = Path(__file__).resolve().parents[1] / "measurement_validation.py"
spec = importlib.util.spec_from_file_location("measurement_validation", MODULE_PATH)
measurement_validation = importlib.util.module_from_spec(spec)
sys.modules["measurement_validation"] = measurement_validation
spec.loader.exec_module(measurement_validation)


def valid_record(**overrides):
    record = {
        "record_schema_version": "1.0.0",
        "pr_number": 358,
        "period": "2026-09",
        "outcome": "merged",
        "hours_open_to_conclusion": 6,
        "review_rounds": 1,
        "complexity": {
            "computed": "simple",
            "final": "simple",
            "override": False,
            "override_reason": None,
        },
        "rubric": {
            "effectful": False,
            "dependency_count": 0,
            "changed_specs": False,
            "added_persona": False,
            "schema_field_count": 12,
            "schema_field_threshold": 20,
        },
        "ci_failure_counts": {"schema": 1, "spec_alignment": 0},
    }
    record.update(overrides)
    return record


def write_record(tmp_dir: str, record: dict, name: str = "pr-358.json") -> Path:
    path = Path(tmp_dir) / name
    path.write_text(json.dumps(record))
    return path


class RubricVerdictTests(unittest.TestCase):
    def test_all_conditions_met_is_simple(self):
        self.assertEqual(
            measurement_validation.rubric_verdict(valid_record()["rubric"]), "simple"
        )

    def test_effectful_forces_complex(self):
        r = valid_record()["rubric"]
        r["effectful"] = True
        self.assertEqual(measurement_validation.rubric_verdict(r), "complex")

    def test_dependency_forces_complex(self):
        r = valid_record()["rubric"]
        r["dependency_count"] = 1
        self.assertEqual(measurement_validation.rubric_verdict(r), "complex")

    def test_changed_specs_forces_complex(self):
        r = valid_record()["rubric"]
        r["changed_specs"] = True
        self.assertEqual(measurement_validation.rubric_verdict(r), "complex")

    def test_added_persona_forces_complex(self):
        r = valid_record()["rubric"]
        r["added_persona"] = True
        self.assertEqual(measurement_validation.rubric_verdict(r), "complex")

    def test_schema_field_count_over_threshold_forces_complex(self):
        r = valid_record()["rubric"]
        r["schema_field_count"] = 21
        self.assertEqual(measurement_validation.rubric_verdict(r), "complex")

    def test_schema_field_count_at_threshold_is_simple(self):
        r = valid_record()["rubric"]
        r["schema_field_count"] = 20
        self.assertEqual(measurement_validation.rubric_verdict(r), "simple")


class ValidRecordTests(unittest.TestCase):
    def test_clean_record_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_record(tmp, valid_record())
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertEqual(errors, [])

    def test_complex_record_with_matching_rubric_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            record = valid_record()
            record["rubric"]["effectful"] = True
            record["complexity"] = {
                "computed": "complex",
                "final": "complex",
                "override": False,
                "override_reason": None,
            }
            path = write_record(tmp, record)
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertEqual(errors, [])

    def test_valid_override_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            record = valid_record()
            record["complexity"] = {
                "computed": "simple",
                "final": "complex",
                "override": True,
                "override_reason": "small schema but the state machine is subtle",
            }
            path = write_record(tmp, record)
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertEqual(errors, [])


class FilenameAndShapeTests(unittest.TestCase):
    def test_bad_filename_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_record(tmp, valid_record(), name="record-358.json")
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertTrue(any(e["code"] == "record.filename" for e in errors))

    def test_pr_number_must_match_filename(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_record(tmp, valid_record(pr_number=999), name="pr-358.json")
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertTrue(any(e["code"] == "record.pr_number_mismatch" for e in errors))

    def test_unknown_top_level_field_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_record(tmp, valid_record(capability_id="core.foo"))
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertTrue(any(e["code"] == "record.unknown_field" for e in errors))

    def test_missing_field_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            record = valid_record()
            del record["period"]
            path = write_record(tmp, record)
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertTrue(any(e["code"] == "record.missing_field" for e in errors))

    def test_invalid_json_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "pr-358.json"
            path.write_text("{not json")
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertTrue(any(e["code"] == "record.invalid_json" for e in errors))

    def test_bad_outcome_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_record(tmp, valid_record(outcome="abandoned"))
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertTrue(any(e["code"] == "record.outcome" for e in errors))

    def test_negative_hours_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_record(tmp, valid_record(hours_open_to_conclusion=-3))
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertTrue(any(e["code"] == "record.hours" for e in errors))

    def test_boolean_is_not_accepted_as_integer(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_record(tmp, valid_record(review_rounds=True))
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertTrue(any(e["code"] == "record.review_rounds" for e in errors))


class RedactionTests(unittest.TestCase):
    def test_identity_key_at_depth_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            record = valid_record()
            record["rubric"]["author_note"] = "x"
            path = write_record(tmp, record)
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertTrue(any(e["code"] == "record.identity_key" for e in errors))

    def test_email_in_override_reason_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            record = valid_record()
            record["complexity"] = {
                "computed": "simple",
                "final": "complex",
                "override": True,
                "override_reason": "ask alice@example.com about the schema",
            }
            path = write_record(tmp, record)
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertTrue(any(e["code"] == "record.identity_value" for e in errors))

    def test_at_handle_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            record = valid_record()
            record["complexity"] = {
                "computed": "simple",
                "final": "complex",
                "override": True,
                "override_reason": "flagged by @reviewer in passing",
            }
            path = write_record(tmp, record)
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertTrue(any(e["code"] == "record.identity_value" for e in errors))

    def test_url_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            record = valid_record()
            record["complexity"] = {
                "computed": "simple",
                "final": "complex",
                "override": True,
                "override_reason": "see https://example.com/thread for context",
            }
            path = write_record(tmp, record)
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertTrue(any(e["code"] == "record.identity_value" for e in errors))

    def test_day_precision_period_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_record(tmp, valid_record(period="2026-09-06"))
            errors: list = []
            measurement_validation.validate_record(path, errors)
            codes = {e["code"] for e in errors}
            self.assertIn("record.period", codes)
            self.assertIn("record.fine_timestamp", codes)

    def test_iso_instant_anywhere_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            record = valid_record()
            record["complexity"] = {
                "computed": "simple",
                "final": "complex",
                "override": True,
                "override_reason": "stalled 2026-08-15T09:00:00Z pending an answer",
            }
            path = write_record(tmp, record)
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertTrue(any(e["code"] == "record.fine_timestamp" for e in errors))


class OverrideConsistencyTests(unittest.TestCase):
    def test_override_false_with_diverging_final_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            record = valid_record()
            record["complexity"]["final"] = "complex"
            path = write_record(tmp, record)
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertTrue(any(e["code"] == "record.override_inconsistent" for e in errors))

    def test_override_false_with_reason_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            record = valid_record()
            record["complexity"]["override_reason"] = "should be null"
            path = write_record(tmp, record)
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertTrue(any(e["code"] == "record.override_inconsistent" for e in errors))

    def test_override_true_with_equal_final_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            record = valid_record()
            record["complexity"] = {
                "computed": "simple",
                "final": "simple",
                "override": True,
                "override_reason": "changed my mind but kept the value",
            }
            path = write_record(tmp, record)
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertTrue(any(e["code"] == "record.override_inconsistent" for e in errors))

    def test_override_true_with_empty_reason_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            record = valid_record()
            record["complexity"] = {
                "computed": "simple",
                "final": "complex",
                "override": True,
                "override_reason": "",
            }
            path = write_record(tmp, record)
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertTrue(any(e["code"] == "record.override_reason" for e in errors))

    def test_override_true_with_oversized_reason_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            record = valid_record()
            record["complexity"] = {
                "computed": "simple",
                "final": "complex",
                "override": True,
                "override_reason": "x" * 201,
            }
            path = write_record(tmp, record)
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertTrue(any(e["code"] == "record.override_reason" for e in errors))


class RubricMismatchTests(unittest.TestCase):
    def test_rubric_says_complex_but_computed_simple_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            record = valid_record()
            record["rubric"]["dependency_count"] = 2  # implies complex
            path = write_record(tmp, record)
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertTrue(any(e["code"] == "record.rubric_mismatch" for e in errors))

    def test_rubric_says_simple_but_computed_complex_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            record = valid_record()
            record["complexity"] = {
                "computed": "complex",
                "final": "complex",
                "override": False,
                "override_reason": None,
            }
            path = write_record(tmp, record)
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertTrue(any(e["code"] == "record.rubric_mismatch" for e in errors))

    def test_unknown_rubric_field_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            record = valid_record()
            record["rubric"]["extra"] = 1
            path = write_record(tmp, record)
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertTrue(any(e["code"] == "record.rubric_unknown_field" for e in errors))


class CiFailureCountsTests(unittest.TestCase):
    def test_unknown_category_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_record(tmp, valid_record(ci_failure_counts={"typo_category": 1}))
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertTrue(
                any(e["code"] == "record.ci_failure_unknown_category" for e in errors)
            )

    def test_negative_count_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_record(tmp, valid_record(ci_failure_counts={"schema": -1}))
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertTrue(any(e["code"] == "record.ci_failure_count_type" for e in errors))

    def test_empty_counts_object_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_record(tmp, valid_record(ci_failure_counts={}))
            errors: list = []
            measurement_validation.validate_record(path, errors)
            self.assertEqual(errors, [])


class ImmutabilityTests(unittest.TestCase):
    def test_modified_record_rejected(self):
        errors: list = []
        with patch.object(
            measurement_validation.subprocess,
            "check_output",
            return_value="M\tmetrics/publication-lifecycle/pr-100.json\n",
        ):
            measurement_validation.check_record_immutability("base", "head", errors)
        self.assertTrue(any(e["code"] == "record.immutable" for e in errors))

    def test_deleted_record_rejected(self):
        errors: list = []
        with patch.object(
            measurement_validation.subprocess,
            "check_output",
            return_value="D\tmetrics/publication-lifecycle/pr-100.json\n",
        ):
            measurement_validation.check_record_immutability("base", "head", errors)
        self.assertTrue(any(e["code"] == "record.immutable" for e in errors))

    def test_added_record_allowed(self):
        errors: list = []
        with patch.object(
            measurement_validation.subprocess,
            "check_output",
            return_value="A\tmetrics/publication-lifecycle/pr-101.json\n",
        ):
            measurement_validation.check_record_immutability("base", "head", errors)
        self.assertEqual(errors, [])

    def test_schema_change_not_treated_as_record(self):
        errors: list = []
        with patch.object(
            measurement_validation.subprocess,
            "check_output",
            return_value="M\tmetrics/publication-lifecycle/schema.json\n",
        ):
            measurement_validation.check_record_immutability("base", "head", errors)
        self.assertEqual(errors, [])


class MainTests(unittest.TestCase):
    def test_main_passes_on_clean_tree(self):
        with tempfile.TemporaryDirectory() as tmp:
            records = Path(tmp) / "metrics" / "publication-lifecycle"
            records.mkdir(parents=True)
            (records / "pr-358.json").write_text(json.dumps(valid_record()))
            with patch.object(measurement_validation, "RECORDS_DIR", records), patch.object(
                sys, "argv", ["measurement_validation.py"]
            ), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(measurement_validation.main(), 0)

    def test_main_fails_on_bad_record(self):
        with tempfile.TemporaryDirectory() as tmp:
            records = Path(tmp) / "metrics" / "publication-lifecycle"
            records.mkdir(parents=True)
            (records / "pr-358.json").write_text(json.dumps(valid_record(outcome="nope")))
            with patch.object(measurement_validation, "RECORDS_DIR", records), patch.object(
                sys, "argv", ["measurement_validation.py"]
            ), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(measurement_validation.main(), 1)

    def test_main_missing_dir_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            records = Path(tmp) / "metrics" / "publication-lifecycle"
            with patch.object(measurement_validation, "RECORDS_DIR", records), patch.object(
                sys, "argv", ["measurement_validation.py"]
            ), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(measurement_validation.main(), 0)


if __name__ == "__main__":
    unittest.main()
