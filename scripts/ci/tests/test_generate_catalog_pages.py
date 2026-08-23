#!/usr/bin/env python3
"""Unit tests for the artifact CORS-mirror link helper (registry#304)."""

import importlib.util
import os
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
MODULE_PATH = REPO_ROOT / "scripts" / "ci" / "generate_catalog_pages.py"
os.chdir(REPO_ROOT)


def load_module():
    spec = importlib.util.spec_from_file_location("generate_catalog_pages", MODULE_PATH)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


class ArtifactMirrorUrlTests(unittest.TestCase):
    def setUp(self):
        self.mod = load_module()

    def test_recognized_release_url_gets_mirror(self):
        url = self.mod.artifact_mirror_url(
            "https://registry.traverse-framework.com",
            "https://github.com/traverse-framework/registry/releases/download/artifacts/core.foo-1.0.0/core-foo.wasm",
        )
        self.assertEqual(
            url,
            "https://registry.traverse-framework.com/artifacts/core.foo-1.0.0/core-foo.wasm",
        )

    def test_unrecognized_url_returns_none(self):
        url = self.mod.artifact_mirror_url(
            "https://registry.traverse-framework.com",
            "https://example.com/some/other/path.wasm",
        )
        self.assertIsNone(url)


if __name__ == "__main__":
    unittest.main()
