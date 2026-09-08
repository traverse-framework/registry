#!/usr/bin/env python3
"""Unit tests for scripts/ci/catalog_coverage_check.py (decision-log entry 95).

The check must:
  * pass on the real repo tree (no retroactive failures);
  * flag a capability-src/ crate that no published id resolves to;
  * flag a current, non-deprecated capability that resolves to no crate;
  * stay quiet for a deprecated-only id, and for an id listed in
    KNOWN_SOURCELESS.
"""

import importlib.util
import json
import os
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

REPO_ROOT = Path(__file__).resolve().parents[3]
CHECK_PATH = REPO_ROOT / "scripts" / "ci" / "catalog_coverage_check.py"


def load_check():
    spec = importlib.util.spec_from_file_location("catalog_coverage_check", CHECK_PATH)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def write_contract(root: Path, namespace: str, cid: str, version: str, *, deprecated=False):
    d = root / "capabilities" / namespace / cid / version
    d.mkdir(parents=True, exist_ok=True)
    (d / "contract.json").write_text(json.dumps({"id": cid, "version": version}))
    if deprecated:
        (d / "deprecated.json").write_text("{}")


def write_crate(root: Path, name: str):
    d = root / "capability-src" / name
    d.mkdir(parents=True, exist_ok=True)
    (d / "Cargo.toml").write_text("[package]\nname = \"x\"\n")


class CatalogCoverageCheckTests(unittest.TestCase):
    def setUp(self):
        self.mod = load_check()
        self.gather = self.mod._load_gather_module()

    def test_real_repo_tree_passes(self):
        cwd = os.getcwd()
        os.chdir(REPO_ROOT)
        try:
            self.assertEqual(self.mod.find_failures(self.gather), [])
        finally:
            os.chdir(cwd)

    def _run_in_fixture(self, build):
        cwd = os.getcwd()
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            build(root)
            os.chdir(tmp)
            try:
                return self.mod.find_failures(self.gather)
            finally:
                os.chdir(cwd)

    def test_matched_id_and_canonical_crate_pass(self):
        def build(root):
            write_contract(root, "commerce", "commerce.thing-happened", "1.0.0")
            write_crate(root, "commerce-thing-happened")  # canonical name
        self.assertEqual(self._run_in_fixture(build), [])

    def test_unreferenced_crate_is_flagged(self):
        def build(root):
            write_contract(root, "commerce", "commerce.thing-happened", "1.0.0")
            write_crate(root, "commerce-thing-happened")
            write_crate(root, "orphaned-crate")  # nothing resolves here
        failures = self._run_in_fixture(build)
        self.assertEqual(len(failures), 1)
        self.assertEqual(failures[0]["code"], "catalog.unreferenced_capability_src_crate")
        self.assertIn("orphaned-crate", failures[0]["path"])

    def test_non_capability_crates_are_ignored(self):
        def build(root):
            write_contract(root, "commerce", "commerce.thing-happened", "1.0.0")
            write_crate(root, "commerce-thing-happened")
            write_crate(root, "wasi-capability-runtime")
            write_crate(root, "catalog-builder")
        self.assertEqual(self._run_in_fixture(build), [])

    def test_current_capability_without_source_is_flagged(self):
        def build(root):
            write_contract(root, "commerce", "commerce.thing-happened", "1.0.0")
            # no crate at all
        failures = self._run_in_fixture(build)
        self.assertEqual(len(failures), 1)
        self.assertEqual(failures[0]["code"], "catalog.current_capability_without_source")

    def test_deprecated_only_id_without_source_is_not_flagged(self):
        def build(root):
            write_contract(root, "commerce", "commerce.old-thing", "1.0.0", deprecated=True)
        self.assertEqual(self._run_in_fixture(build), [])

    def test_known_sourceless_exception_is_respected(self):
        original = set(self.mod.KNOWN_SOURCELESS)
        self.mod.KNOWN_SOURCELESS.add("commerce.thing-happened")
        try:
            def build(root):
                write_contract(root, "commerce", "commerce.thing-happened", "1.0.0")
            self.assertEqual(self._run_in_fixture(build), [])
        finally:
            self.mod.KNOWN_SOURCELESS.clear()
            self.mod.KNOWN_SOURCELESS.update(original)

    def test_latest_version_selection_uses_semver_not_string_order(self):
        def build(root):
            write_contract(root, "commerce", "commerce.thing-happened", "1.9.0")
            write_contract(root, "commerce", "commerce.thing-happened", "1.10.0", deprecated=True)
            write_crate(root, "commerce-thing-happened")
        # 1.10.0 > 1.9.0 numerically and it is deprecated -> the current
        # version is deprecated -> no "without source" complaint, and the
        # crate is still referenced so no orphan complaint.
        self.assertEqual(self._run_in_fixture(build), [])


if __name__ == "__main__":
    unittest.main()
