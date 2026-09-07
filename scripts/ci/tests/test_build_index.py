#!/usr/bin/env python3
"""Tests for specs/009-contract-metadata-in-index (Draft) FR-001 through
FR-004 in scripts/ci/build_index.py (registry issue #44).

Run with: python3 -m unittest scripts/ci/tests/test_build_index.py
"""

import hashlib
import importlib.util
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).resolve().parents[1] / "build_index.py"
spec = importlib.util.spec_from_file_location("build_index", MODULE_PATH)
build_index_module = importlib.util.module_from_spec(spec)
sys.modules["build_index"] = build_index_module
spec.loader.exec_module(build_index_module)


def write_contract(tmp_dir: str, contract: dict, namespace="core", cap_id="example-capability", version="1.0.0") -> Path:
    path = Path(tmp_dir) / "capabilities" / namespace / cap_id / version / "contract.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(contract))
    return path


def valid_contract():
    return {
        "id": "example-capability",
        "namespace": "core",
        "owner": {"team": "platform"},
        "version": "1.0.0",
        "artifact": {"digest": "sha256:abc123", "url": "https://example.invalid/artifact.wasm"},
    }


class BuildIndexContractMetadataTests(unittest.TestCase):
    def _run_in(self, tmp_dir: str, *args, **kwargs):
        cwd = os.getcwd()
        os.chdir(tmp_dir)
        try:
            return build_index_module.build_index(*args, **kwargs)
        finally:
            os.chdir(cwd)

    def test_entry_includes_contract_digest_and_url(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_contract(tmp, valid_contract())
            raw_bytes = path.read_bytes()
            expected_digest = f"sha256:{hashlib.sha256(raw_bytes).hexdigest()}"

            index = self._run_in(tmp, 0, "deadbeef", "traverse-framework/registry")

            self.assertEqual(len(index["capabilities"]), 1)
            entry = index["capabilities"][0]
            self.assertEqual(entry["contract_digest"], expected_digest)
            self.assertEqual(
                entry["contract_url"],
                "https://raw.githubusercontent.com/traverse-framework/registry/deadbeef/"
                "capabilities/core/example-capability/1.0.0/contract.json",
            )

    def test_unreadable_contract_aborts_build(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_contract(tmp, valid_contract())
            path.write_text("{not valid json")

            with self.assertRaises(build_index_module.IndexBuildError) as ctx:
                self._run_in(tmp, 0, "deadbeef")

            self.assertEqual(ctx.exception.code, "index.contract_unreadable")

    def test_yanked_version_retains_contract_fields(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_contract(tmp, valid_contract())
            (path.parent / "deprecated.json").write_text(json.dumps({"reason": "test"}))

            index = self._run_in(tmp, 0, "deadbeef")

            entry = index["capabilities"][0]
            self.assertTrue(entry["deprecated"])
            self.assertIsNotNone(entry["contract_digest"])
            self.assertIsNotNone(entry["contract_url"])

    def test_contract_digest_independently_verifiable(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_contract(tmp, valid_contract())
            index = self._run_in(tmp, 0, "deadbeef")
            entry = index["capabilities"][0]

            recomputed = f"sha256:{hashlib.sha256(path.read_bytes()).hexdigest()}"
            self.assertEqual(entry["contract_digest"], recomputed)

    def test_index_version_increments(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_contract(tmp, valid_contract())
            index = self._run_in(tmp, 5, "deadbeef")
            self.assertEqual(index["index_version"], 6)

    def test_entry_carries_sanitized_display_metadata(self):
        # specs/019-public-metadata-sync-extension FR-001/FR-002
        # (registry#312): summary/description pass through verbatim,
        # use_cases keeps only scenario text.
        with tempfile.TemporaryDirectory() as tmp:
            contract = valid_contract()
            contract["summary"] = "Does a thing."
            contract["description"] = "Does the thing in detail."
            contract["use_cases"] = [
                {
                    "scenario": "As a user, I want X, so that Y.",
                    "input_example": {"secret": "should not leak"},
                    "output_example": {"also_secret": "should not leak"},
                    "happy": True,
                    "persona_ref": "some-persona@1.0.0",
                }
            ]
            write_contract(tmp, contract)

            index = self._run_in(tmp, 0, "deadbeef")

            entry = index["capabilities"][0]
            self.assertEqual(entry["summary"], "Does a thing.")
            self.assertEqual(entry["description"], "Does the thing in detail.")
            self.assertEqual(entry["use_cases"], [{"scenario": "As a user, I want X, so that Y."}])

    def test_missing_display_metadata_does_not_fail_build(self):
        # FR-003: absence is valid, not a build failure -- matches this
        # script's existing non-retroactive stance on older content.
        with tempfile.TemporaryDirectory() as tmp:
            write_contract(tmp, valid_contract())
            index = self._run_in(tmp, 0, "deadbeef")

            entry = index["capabilities"][0]
            self.assertEqual(entry["summary"], "")
            self.assertEqual(entry["description"], "")
            self.assertEqual(entry["use_cases"], [])

    def test_entry_carries_search_projection_fields(self):
        # specs/019-public-metadata-sync-extension amendment FR-006/FR-007
        # (registry#318): service_type/permitted_targets/lifecycle copied
        # verbatim, provenance passed through unfiltered (no redaction --
        # unlike use_cases, none of its fields are secret or PII).
        with tempfile.TemporaryDirectory() as tmp:
            contract = valid_contract()
            contract["service_type"] = "stateless"
            contract["permitted_targets"] = ["local", "cloud"]
            contract["lifecycle"] = "active"
            contract["provenance"] = {
                "source": "greenfield",
                "author": "enricopiovesan",
                "created_at": "2026-07-08T00:00:00Z",
                "spec_ref": "058-workflow-pipeline-execution@1.0.0",
                "adr_refs": ["0001-rust-wasm-foundation"],
                "exception_refs": [],
            }
            write_contract(tmp, contract)

            index = self._run_in(tmp, 0, "deadbeef")

            entry = index["capabilities"][0]
            self.assertEqual(entry["service_type"], "stateless")
            self.assertEqual(entry["permitted_targets"], ["local", "cloud"])
            self.assertEqual(entry["lifecycle"], "active")
            self.assertEqual(entry["provenance"], contract["provenance"])

    def test_missing_search_projection_fields_does_not_fail_build(self):
        # FR-008: absence is valid, not a build failure.
        with tempfile.TemporaryDirectory() as tmp:
            write_contract(tmp, valid_contract())
            index = self._run_in(tmp, 0, "deadbeef")

            entry = index["capabilities"][0]
            self.assertEqual(entry["service_type"], "")
            self.assertEqual(entry["permitted_targets"], [])
            self.assertEqual(entry["lifecycle"], "")
            self.assertIsNone(entry["provenance"])

    def test_active_contract_missing_artifact_aborts_build(self):
        # Regression test for a real incident (registry#89/#90): an active
        # contract with no artifact.digest/.url must never reach the
        # index with null fields -- that crashes every consumer's parse
        # of the whole index, not just this one record.
        with tempfile.TemporaryDirectory() as tmp:
            contract = valid_contract()
            del contract["artifact"]
            write_contract(tmp, contract)

            with self.assertRaises(build_index_module.IndexBuildError) as ctx:
                self._run_in(tmp, 0, "deadbeef")

            self.assertEqual(ctx.exception.code, "index.missing_artifact_reference")

    def test_deprecated_contract_missing_artifact_is_excluded_not_failed(self):
        # Contracts are immutable, so an already-published, already-broken
        # deprecated version can never be fixed by editing -- excluding it
        # from the index is the only way a future build can ever succeed
        # again. The build must not fail, and the record must not appear.
        with tempfile.TemporaryDirectory() as tmp:
            contract = valid_contract()
            del contract["artifact"]
            path = write_contract(tmp, contract)
            (path.parent / "deprecated.json").write_text(json.dumps({"reason": "test"}))

            index = self._run_in(tmp, 0, "deadbeef")

            self.assertEqual(index["capabilities"], [])

    def test_deprecated_contract_with_artifact_is_still_included(self):
        # Sanity check the exclusion is specifically about missing
        # artifact info, not deprecation itself.
        with tempfile.TemporaryDirectory() as tmp:
            write_contract(tmp, valid_contract())
            path = Path(tmp) / "capabilities" / "core" / "example-capability" / "1.0.0"
            (path / "deprecated.json").write_text(json.dumps({"reason": "test"}))

            index = self._run_in(tmp, 0, "deadbeef")

            self.assertEqual(len(index["capabilities"]), 1)
            self.assertTrue(index["capabilities"][0]["deprecated"])


def write_workflow(tmp_dir: str, workflow: dict, namespace="core", workflow_id="example.workflow", version="1.0.0") -> Path:
    path = Path(tmp_dir) / "workflows" / namespace / workflow_id / version / "workflow.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(workflow))
    return path


def valid_workflow():
    return {"id": "example.workflow", "namespace": "core", "version": "1.0.0", "nodes": []}


class BuildIndexWorkflowFR013Tests(unittest.TestCase):
    """registry#124: workflows[] array, spec 001 FR-013."""

    def _run_in(self, tmp_dir: str, *args, **kwargs):
        cwd = os.getcwd()
        os.chdir(tmp_dir)
        try:
            return build_index_module.build_index(*args, **kwargs)
        finally:
            os.chdir(cwd)

    def test_entry_includes_workflow_digest_and_url(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_workflow(tmp, valid_workflow())
            raw_bytes = path.read_bytes()
            expected_digest = f"sha256:{hashlib.sha256(raw_bytes).hexdigest()}"

            index = self._run_in(tmp, 0, "deadbeef", "traverse-framework/registry")

            self.assertEqual(len(index["workflows"]), 1)
            entry = index["workflows"][0]
            self.assertEqual(entry["workflow_digest"], expected_digest)
            self.assertEqual(
                entry["workflow_url"],
                "https://raw.githubusercontent.com/traverse-framework/registry/deadbeef/"
                "workflows/core/example.workflow/1.0.0/workflow.json",
            )
            self.assertEqual(entry["namespace"], "core")
            self.assertEqual(entry["id"], "example.workflow")
            self.assertEqual(entry["version"], "1.0.0")
            self.assertFalse(entry["deprecated"])

    def test_examples_subtree_is_excluded(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "workflows" / "examples" / "expedition" / "plan-expedition" / "workflow.json"
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(json.dumps({"id": "expedition.planning.plan-expedition", "namespace": "expedition.planning", "version": "1.0.0"}))

            index = self._run_in(tmp, 0, "deadbeef")

            self.assertEqual(index["workflows"], [])

    def test_deprecated_workflow_is_flagged_not_excluded(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_workflow(tmp, valid_workflow())
            path = Path(tmp) / "workflows" / "core" / "example.workflow" / "1.0.0"
            (path / "deprecated.json").write_text(json.dumps({"reason": "test"}))

            index = self._run_in(tmp, 0, "deadbeef")

            self.assertEqual(len(index["workflows"]), 1)
            self.assertTrue(index["workflows"][0]["deprecated"])

    def test_unreadable_workflow_aborts_build(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "workflows" / "core" / "example.workflow" / "1.0.0" / "workflow.json"
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("{ not valid json")

            with self.assertRaises(build_index_module.IndexBuildError) as ctx:
                self._run_in(tmp, 0, "deadbeef")
            self.assertEqual(ctx.exception.code, "index.workflow_unreadable")

    def test_capabilities_and_workflows_coexist_independently(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_contract(tmp, valid_contract())
            write_workflow(tmp, valid_workflow())

            index = self._run_in(tmp, 0, "deadbeef")

            self.assertEqual(len(index["capabilities"]), 1)
            self.assertEqual(len(index["workflows"]), 1)


def write_event_product(
    tmp_dir: str,
    product: dict,
    namespace="core",
    event_id="core.example.event-created",
    version="1.0.0",
) -> Path:
    path = Path(tmp_dir) / "events" / namespace / event_id / version / "product.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(product))
    return path


def valid_event_product():
    return {
        "contract": {
            "id": "core.example.event-created",
            "namespace": "core",
            "version": "1.0.0",
            "summary": "Example event",
        },
        "exposure": "internal",
    }


class BuildIndexEventFR016Tests(unittest.TestCase):
    """registry#168: events[] array, spec 001 FR-016."""

    def _run_in(self, tmp_dir: str, *args, **kwargs):
        cwd = os.getcwd()
        os.chdir(tmp_dir)
        try:
            return build_index_module.build_index(*args, **kwargs)
        finally:
            os.chdir(cwd)

    def test_entry_includes_product_digest_and_url(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_event_product(tmp, valid_event_product())
            raw_bytes = path.read_bytes()
            expected_digest = f"sha256:{hashlib.sha256(raw_bytes).hexdigest()}"

            index = self._run_in(tmp, 0, "deadbeef", "traverse-framework/registry")

            self.assertEqual(len(index["events"]), 1)
            entry = index["events"][0]
            self.assertEqual(entry["product_digest"], expected_digest)
            self.assertEqual(
                entry["product_url"],
                "https://raw.githubusercontent.com/traverse-framework/registry/deadbeef/"
                "events/core/core.example.event-created/1.0.0/product.json",
            )
            self.assertEqual(entry["namespace"], "core")
            self.assertEqual(entry["id"], "core.example.event-created")
            self.assertEqual(entry["version"], "1.0.0")
            self.assertFalse(entry["deprecated"])

    def test_deprecated_event_is_flagged(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_event_product(tmp, valid_event_product())
            (path.parent / "deprecated.json").write_text(json.dumps({"reason": "test"}))

            index = self._run_in(tmp, 0, "deadbeef")

            self.assertEqual(len(index["events"]), 1)
            self.assertTrue(index["events"][0]["deprecated"])

    def test_unreadable_event_product_aborts_build(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "events" / "core" / "core.example.event-created" / "1.0.0" / "product.json"
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("{ not valid json")

            with self.assertRaises(build_index_module.IndexBuildError) as ctx:
                self._run_in(tmp, 0, "deadbeef")
            self.assertEqual(ctx.exception.code, "index.event_product_unreadable")

    def test_capabilities_workflows_and_events_coexist(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_contract(tmp, valid_contract())
            write_workflow(tmp, valid_workflow())
            write_event_product(tmp, valid_event_product())

            index = self._run_in(tmp, 0, "deadbeef")

            self.assertEqual(len(index["capabilities"]), 1)
            self.assertEqual(len(index["workflows"]), 1)
            self.assertEqual(len(index["events"]), 1)


if __name__ == "__main__":
    unittest.main()
