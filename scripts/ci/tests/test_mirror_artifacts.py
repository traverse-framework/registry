#!/usr/bin/env python3
"""Unit tests for the artifact-mirroring CI step (registry#304, registry#383)."""

import hashlib
import importlib.util
import json
import os
import sys
import tempfile
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


class ContractMirrorTests(unittest.TestCase):
    """registry#383: verbatim contract file + registry-authored digest, on the
    CORS-open Pages mirror, referenced from catalog.json."""

    def setUp(self):
        self.mod = load_module()

    WASM = b"fake wasm bytes"
    URL = (
        "https://github.com/traverse-framework/registry/releases/download/"
        "artifacts/core.foo-1.0.0/core-foo.wasm"
    )

    def _write_contract(self, tmp_path: Path) -> bytes:
        contract_dir = tmp_path / "capabilities" / "core" / "core.foo" / "1.0.0"
        contract_dir.mkdir(parents=True)
        # Deliberately not re-serialized here: the mirror must copy whatever
        # bytes are on disk, and the digest must be over those exact bytes.
        raw = (
            b'{\n  "namespace": "core",\n  "id": "core.foo",\n  "version": "1.0.0",\n'
            b'  "artifact": {"digest": "%s", "url": "%s"}\n}\n'
            % (
                f"sha256:{hashlib.sha256(self.WASM).hexdigest()}".encode(),
                self.URL.encode(),
            )
        )
        (contract_dir / "contract.json").write_bytes(raw)
        return raw

    def _run(self, tmp_path: Path, out_dir: Path, argv_extra=None):
        argv = ["mirror_artifacts.py", str(out_dir), *(argv_extra or [])]
        with mock.patch.object(self.mod, "ROOT", tmp_path), mock.patch.object(
            self.mod, "fetch", return_value=self.WASM
        ):
            return self.mod.main(argv)

    def test_contract_is_mirrored_verbatim_with_digest_sibling(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            raw = self._write_contract(tmp_path)
            out_dir = tmp_path / "catalog"

            self.assertEqual(self._run(tmp_path, out_dir), 0)

            mirrored = out_dir / "artifacts" / "core.foo-1.0.0" / "contract.json"
            digest_sibling = out_dir / "artifacts" / "core.foo-1.0.0" / "contract.json.sha256"
            self.assertEqual(mirrored.read_bytes(), raw)
            # Same recipe build_index.py uses for index.json's contract_digest.
            self.assertEqual(
                digest_sibling.read_text().strip(),
                f"sha256:{hashlib.sha256(raw).hexdigest()}",
            )

    def test_catalog_json_entry_gains_contract_url_and_digest(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            raw = self._write_contract(tmp_path)
            out_dir = tmp_path / "catalog"
            out_dir.mkdir()
            (out_dir / "catalog.json").write_text(
                json.dumps(
                    {
                        "capabilities": [
                            {"reference": "core/core.foo@1.0.0", "deprecated": False, "contract": {}},
                            {"reference": "core/core.bar@2.0.0", "deprecated": False, "contract": {}},
                        ]
                    }
                )
            )

            self.assertEqual(
                self._run(tmp_path, out_dir, ["https://example.test/"]), 0
            )

            catalog = json.loads((out_dir / "catalog.json").read_text())
            foo = next(c for c in catalog["capabilities"] if c["reference"] == "core/core.foo@1.0.0")
            bar = next(c for c in catalog["capabilities"] if c["reference"] == "core/core.bar@2.0.0")
            self.assertEqual(
                foo["contract_url"],
                "https://example.test/artifacts/core.foo-1.0.0/contract.json",
            )
            self.assertEqual(
                foo["contract_digest"], f"sha256:{hashlib.sha256(raw).hexdigest()}"
            )
            # An entry with no mirrored contract is left untouched.
            self.assertNotIn("contract_url", bar)
            self.assertNotIn("contract_digest", bar)

    def test_missing_catalog_json_is_tolerated(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            self._write_contract(tmp_path)
            out_dir = tmp_path / "catalog"

            self.assertEqual(self._run(tmp_path, out_dir), 0)
            self.assertTrue((out_dir / "artifacts" / "core.foo-1.0.0" / "contract.json").exists())
            self.assertFalse((out_dir / "catalog.json").exists())

    def test_rejects_too_many_args(self):
        self.assertEqual(
            self.mod.main(["mirror_artifacts.py", "catalog", "https://x", "extra"]), 2
        )

    def test_two_versions_sharing_one_artifact_release_each_get_their_own_mirror(self):
        """registry#421: a v1.1.0 that reuses v1.0.0's artifact release must
        still get its own artifacts/<ns>.<id>-1.1.0/contract.json -- keyed on
        the contract's identity, not artifact.url's -1.0.0 tag."""
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            digest = f"sha256:{hashlib.sha256(self.WASM).hexdigest()}"
            for ver, marker in (("1.0.0", "old"), ("1.1.0", "new")):
                cdir = tmp_path / "capabilities" / "core" / "core.foo" / ver
                cdir.mkdir(parents=True)
                (cdir / "contract.json").write_bytes(
                    (
                        '{"namespace":"core","id":"core.foo","version":"%s","marker":"%s",'
                        '"artifact":{"digest":"%s","url":"%s"}}\n'
                        % (ver, marker, digest, self.URL)  # both point at the -1.0.0 release
                    ).encode()
                )
            out_dir = tmp_path / "catalog"
            out_dir.mkdir()
            (out_dir / "catalog.json").write_text(
                json.dumps(
                    {
                        "capabilities": [
                            {"reference": "core/core.foo@1.0.0", "deprecated": True, "contract": {}},
                            {"reference": "core/core.foo@1.1.0", "deprecated": False, "contract": {}},
                        ]
                    }
                )
            )

            self.assertEqual(self._run(tmp_path, out_dir, ["https://example.test/"]), 0)

            old = out_dir / "artifacts" / "core.foo-1.0.0" / "contract.json"
            new = out_dir / "artifacts" / "core.foo-1.1.0" / "contract.json"
            self.assertTrue(old.exists() and new.exists())
            self.assertEqual(json.loads(old.read_text())["marker"], "old")
            self.assertEqual(json.loads(new.read_text())["marker"], "new")

            catalog = json.loads((out_dir / "catalog.json").read_text())
            by_ref = {c["reference"]: c for c in catalog["capabilities"]}
            self.assertEqual(
                by_ref["core/core.foo@1.0.0"]["contract_url"],
                "https://example.test/artifacts/core.foo-1.0.0/contract.json",
            )
            self.assertEqual(
                by_ref["core/core.foo@1.1.0"]["contract_url"],
                "https://example.test/artifacts/core.foo-1.1.0/contract.json",
            )


if __name__ == "__main__":
    unittest.main()
