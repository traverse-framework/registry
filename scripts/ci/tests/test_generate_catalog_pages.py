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


class AiFieldHtmlTests(unittest.TestCase):
    """spec 001 FR-017: the static page's Agent / Models rows."""

    def setUp(self):
        self.mod = load_module()

    def test_absent_or_not_model_backed_renders_nothing(self):
        self.assertEqual(self.mod.ai_field_html(None), "")
        self.assertEqual(self.mod.ai_field_html({"model_backed": False}), "")

    def test_model_backed_renders_agent_and_models_rows(self):
        html = self.mod.ai_field_html(
            {"model_backed": True, "models": ["minishlab/potion-base-32M"]}
        )
        self.assertIn("badge-agent", html)
        self.assertIn("Models", html)
        self.assertIn("minishlab/potion-base-32M", html)


class LicensingSidebarTests(unittest.TestCase):
    """registry#563 / Spec 025: license card on static capability pages."""

    def setUp(self):
        self.mod = load_module()

    def test_absent_licensing_renders_unknown_not_allowed(self):
        html = self.mod.licensing_sidebar_html({"id": "demo.cap"})
        self.assertIn("sidebar-card-title", html)
        self.assertIn(">unknown<", html)
        self.assertIn("deny-by-default", html)
        self.assertNotIn(">allowed<", html)

    def test_present_licensing_projects_rights_and_spdx(self):
        html = self.mod.licensing_sidebar_html(
            {
                "licensing": {
                    "spdx_expression": "MIT",
                    "commercial_use": "allowed",
                    "redistribution": "forbidden",
                    "attribution_required": True,
                    "verification": {"status": "maintainer-declared"},
                }
            }
        )
        self.assertIn("MIT", html)
        self.assertIn("badge-success", html)
        self.assertIn("badge-danger", html)
        self.assertIn("maintainer-declared", html)
        self.assertIn("Attribution", html)
        self.assertIn("Spec 025", html)

    def test_package_sidebar_includes_license_and_package_cards(self):
        entry = {
            "deprecated": False,
            "contract": {
                "namespace": "demo",
                "id": "demo.cap",
                "version": "1.0.0",
                "service_type": "stateless",
                "permitted_targets": ["wasm"],
                "owner": {"team": "traverse-core"},
                "artifact": {
                    "digest": "sha256:abc",
                    "url": "https://github.com/traverse-framework/registry/releases/download/artifacts/demo.cap-1.0.0/demo.wasm",
                },
            },
        }
        html = self.mod.package_sidebar_html(
            entry,
            "https://registry.traverse-framework.com",
            "/capability/demo/demo.cap/1.0.0/",
        )
        self.assertIn("License", html)
        self.assertIn("Package", html)
        self.assertIn("1.0.0", html)
        self.assertIn("CORS mirror", html)
        self.assertIn("deny-by-default", html)


if __name__ == "__main__":
    unittest.main()