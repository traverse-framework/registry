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


if __name__ == "__main__":
    unittest.main()
