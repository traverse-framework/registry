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

    def test_object_shaped_models_still_render_id_in_summary_row(self):
        # spec 001 FR-017 amendment (Decision 124 / registry#571): the
        # "Models" summary row must not go silently empty for the new
        # object-shaped ai.models -- extract the id the same as string[].
        html = self.mod.ai_field_html(
            {
                "model_backed": True,
                "models": [
                    {
                        "id": "dslim/distilbert-NER",
                        "spdx_expression": "Apache-2.0",
                        "attribution_required": False,
                        "source_url": "https://huggingface.co/dslim/distilbert-NER",
                    }
                ],
            }
        )
        self.assertIn("badge-agent", html)
        self.assertIn("Models", html)
        self.assertIn("dslim/distilbert-NER", html)


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



class ModelAttributionSidebarTests(unittest.TestCase):
    """Catalog model-attribution surface (Spec 025 non-inheritance)."""

    def setUp(self):
        self.mod = load_module()
        self.mod._MODEL_ATTRIBUTION_CACHE = None

    def test_whisper_capability_includes_silero_extra(self):
        html = self.mod.model_attribution_sidebar_html(
            {
                "namespace": "audio",
                "id": "audio.transcribe-speech",
                "version": "1.1.0",
                "ai": {"model_backed": True, "models": ["openai/whisper-tiny"]},
            }
        )
        self.assertIn("Model licenses", html)
        self.assertIn("openai/whisper-tiny", html)
        self.assertIn("snakers4/silero-vad", html)
        self.assertIn("MIT OR Apache-2.0", html)
        self.assertIn("allowed", html)
        self.assertIn("Attribution", html)
        self.assertIn("THIRD_PARTY_NOTICES", html)

    def test_detect_entities_shows_distilbert(self):
        html = self.mod.model_attribution_sidebar_html(
            {
                "namespace": "text",
                "id": "text.detect-entities",
                "ai": {"model_backed": True, "models": ["dslim/distilbert-NER"]},
            }
        )
        self.assertIn("dslim/distilbert-NER", html)
        self.assertIn("Apache-2.0", html)

    def test_non_agent_renders_empty(self):
        self.assertEqual(
            self.mod.model_attribution_sidebar_html({"id": "core.authorize"}),
            "",
        )

    def test_object_shaped_model_ref_is_pinned_on_contract_not_looked_up(self):
        # spec 001 FR-017 amendment (Decision 124 / registry#571): an
        # object-shaped ai.models entry is rendered from the contract's own
        # pinned data, never consulting catalog/model-attribution.json.
        html = self.mod.model_attribution_sidebar_html(
            {
                "namespace": "text",
                "id": "text.redact-entities",
                "version": "1.0.0",
                "ai": {
                    "model_backed": True,
                    "models": [
                        {
                            "id": "dslim/distilbert-NER",
                            "spdx_expression": "Apache-2.0",
                            "attribution_required": False,
                            "huggingface_id": "dslim/distilbert-NER",
                            "revision": "cafe123",
                        }
                    ],
                },
            }
        )
        self.assertIn("Model licenses", html)
        self.assertIn("dslim/distilbert-NER", html)
        self.assertIn("Apache-2.0", html)
        self.assertIn("cafe123", html)
        self.assertIn("Pinned on this capability", html)

    def test_object_shaped_model_ref_with_source_url_renders_source_row(self):
        html = self.mod.model_attribution_sidebar_html(
            {
                "namespace": "audio",
                "id": "audio.detect-speech-segments",
                "version": "1.0.0",
                "ai": {
                    "model_backed": True,
                    "models": [
                        {
                            "id": "snakers4/silero-vad",
                            "spdx_expression": "MIT",
                            "attribution_required": True,
                            "source_url": "https://github.com/snakers4/silero-vad",
                        }
                    ],
                },
            }
        )
        self.assertIn("https://github.com/snakers4/silero-vad", html)
        self.assertIn("Attribution", html)

    def test_mixed_legacy_and_object_shapes_both_render(self):
        html = self.mod.model_attribution_sidebar_html(
            {
                "namespace": "core",
                "id": "core.example-multi-model",
                "version": "1.0.0",
                "ai": {
                    "model_backed": True,
                    "models": [
                        "dslim/distilbert-NER",
                        {
                            "id": "openai/whisper-tiny",
                            "spdx_expression": "MIT",
                            "attribution_required": True,
                            "huggingface_id": "openai/whisper-tiny",
                            "revision": "deadbee",
                        },
                    ],
                },
            }
        )
        # legacy entry: looked up in catalog/model-attribution.json
        self.assertIn("Apache-2.0", html)
        # object entry: pinned on the contract itself
        self.assertIn("deadbee", html)
        self.assertIn("Pinned on this capability", html)

    def test_single_model_page_block(self):
        html = self.mod.single_model_attribution_html("openai/whisper-tiny", [])
        self.assertIn("Model license", html)
        self.assertIn("Copyright (c) 2022 OpenAI", html)

    def test_single_model_page_block_prefers_inline_object_ref(self):
        # spec 001 FR-017 amendment (Decision 124 / registry#571): when a
        # matching capability declares this model in the new object shape,
        # that pinned contract data wins over catalog/model-attribution.json.
        matching = [
            {
                "contract": {
                    "ai": {
                        "model_backed": True,
                        "models": [
                            {
                                "id": "openai/whisper-tiny",
                                "spdx_expression": "MIT",
                                "attribution_required": True,
                                "huggingface_id": "openai/whisper-tiny",
                                "revision": "deadbee",
                            }
                        ],
                    }
                }
            }
        ]
        html = self.mod.single_model_attribution_html("openai/whisper-tiny", matching)
        self.assertIn("Model license", html)
        self.assertIn("deadbee", html)
        self.assertIn("Pinned on this capability", html)
        self.assertNotIn("Copyright (c) 2022 OpenAI", html)


if __name__ == "__main__":
    unittest.main()
