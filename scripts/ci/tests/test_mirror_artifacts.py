#!/usr/bin/env python3
"""Unit tests for the artifact-mirroring CI step (registry#304)."""

import importlib.util
import os
import sys
import unittest
from pathlib import Path
from unittest import mock

REPO_ROOT = Path(__file__).resolve().parents[3]
MODULE_PATH = REPO_ROOT / "scripts" / "ci" / "mirror_artifacts.py"
os.chdir(REPO_ROOT)


def load_module():
    spec = importlib.util.spec_from_file_location("mirror_artifacts", MODULE_PATH)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


class MirrorRelpathForUrlTests(unittest.TestCase):
    def setUp(self):
        self.mod = load_module()

    def test_recognized_release_url(self):
        url = "https://github.com/traverse-framework/registry/releases/download/artifacts/core.foo-1.0.0/core-foo.wasm"
        self.assertEqual(
            self.mod.mirror_relpath_for_url(url),
            "artifacts/core.foo-1.0.0/core-foo.wasm",
        )

    def test_wrong_host_rejected(self):
        url = "https://example.com/releases/download/artifacts/core.foo-1.0.0/core-foo.wasm"
        self.assertIsNone(self.mod.mirror_relpath_for_url(url))

    def test_missing_asset_segment_rejected(self):
        url = "https://github.com/traverse-framework/registry/releases/download/artifacts/core.foo-1.0.0"
        self.assertIsNone(self.mod.mirror_relpath_for_url(url))


class MainDigestVerificationTests(unittest.TestCase):
    def setUp(self):
        self.mod = load_module()

    def _contract(self, tmp_path, digest, url):
        contract_dir = tmp_path / "capabilities" / "core" / "core.foo" / "1.0.0"
        contract_dir.mkdir(parents=True)
        contract_path = contract_dir / "contract.json"
        contract_path.write_text('{"artifact": {"digest": "%s", "url": "%s"}}' % (digest, url))
        return contract_path

    def test_matching_digest_writes_mirror(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            body = b"fake wasm bytes"
            import hashlib

            digest = f"sha256:{hashlib.sha256(body).hexdigest()}"
            url = "https://github.com/traverse-framework/registry/releases/download/artifacts/core.foo-1.0.0/core-foo.wasm"
            self._contract(tmp_path, digest, url)

            out_dir = tmp_path / "catalog"
            with mock.patch.object(self.mod, "ROOT", tmp_path), mock.patch.object(self.mod, "fetch", return_value=body):
                rc = self.mod.main(["mirror_artifacts.py", str(out_dir)])

            self.assertEqual(rc, 0)
            mirrored = out_dir / "artifacts" / "core.foo-1.0.0" / "core-foo.wasm"
            self.assertTrue(mirrored.exists())
            self.assertEqual(mirrored.read_bytes(), body)

    def test_digest_mismatch_fails_closed(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            url = "https://github.com/traverse-framework/registry/releases/download/artifacts/core.foo-1.0.0/core-foo.wasm"
            self._contract(tmp_path, "sha256:" + "0" * 64, url)

            out_dir = tmp_path / "catalog"
            with mock.patch.object(self.mod, "ROOT", tmp_path), mock.patch.object(self.mod, "fetch", return_value=b"different bytes"):
                rc = self.mod.main(["mirror_artifacts.py", str(out_dir)])

            self.assertEqual(rc, 1)
            self.assertFalse((out_dir / "artifacts" / "core.foo-1.0.0" / "core-foo.wasm").exists())


if __name__ == "__main__":
    unittest.main()
