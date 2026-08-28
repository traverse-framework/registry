#!/usr/bin/env python3
"""Tests for scripts/ci/sign_artifacts.py (specs/007-artifact-hosting amendment,
registry#334).

Needs the `cryptography` package (CI's sign-artifacts-tests job installs it).

Run with: python3 -m unittest scripts/ci/tests/test_sign_artifacts.py
"""

import importlib.util
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

MODULE_PATH = Path(__file__).resolve().parents[1] / "sign_artifacts.py"
spec = importlib.util.spec_from_file_location("sign_artifacts", MODULE_PATH)
sign_artifacts = importlib.util.module_from_spec(spec)
sys.modules["sign_artifacts"] = sign_artifacts
spec.loader.exec_module(sign_artifacts)

# 32-byte all-0x11 seed -- deterministic, fixed test vector, never a real key.
TEST_SEED_HEX = "11" * 32


def write_version(root: Path, cap_id: str, version: str, *, artifact=True, deprecated=False, signed=False) -> Path:
    version_dir = root / "capabilities" / cap_id.split(".")[0] / cap_id / version
    version_dir.mkdir(parents=True, exist_ok=True)
    contract = {"id": cap_id, "namespace": cap_id.split(".")[0], "version": version}
    if artifact:
        contract["artifact"] = {
            "digest": "sha256:" + "0" * 64,
            "url": "https://github.com/traverse-framework/registry/releases/download/artifacts/x-1.0.0/x.wasm",
        }
    (version_dir / "contract.json").write_text(json.dumps(contract))
    if deprecated:
        (version_dir / "deprecated.json").write_text(json.dumps({"reason": "test"}))
    if signed:
        (version_dir / "signature.json").write_text(json.dumps({"scheme": "ed25519"}))
    return version_dir


class SignerTests(unittest.TestCase):
    def test_public_key_hex_is_32_bytes(self):
        signer = sign_artifacts._load_signer(TEST_SEED_HEX)
        pub = sign_artifacts._public_key_hex(signer)
        self.assertEqual(len(pub), 64)
        self.assertEqual(bytes.fromhex(pub).__len__(), 32)

    def test_signature_verifies_over_exact_bytes(self):
        signer = sign_artifacts._load_signer(TEST_SEED_HEX)
        data = b"the exact artifact bytes"
        sig_hex = sign_artifacts._sign_hex(signer, data)
        self.assertEqual(len(sig_hex), 128)
        pub = Ed25519PublicKey.from_public_bytes(bytes.fromhex(sign_artifacts._public_key_hex(signer)))
        pub.verify(bytes.fromhex(sig_hex), data)  # raises InvalidSignature on failure
        with self.assertRaises(Exception):
            pub.verify(bytes.fromhex(sig_hex), data + b"tampered")

    def test_bad_hex_seed_is_rejected(self):
        with self.assertRaises(SystemExit):
            sign_artifacts._load_signer("nothex!!")

    def test_wrong_length_seed_is_rejected(self):
        with self.assertRaises(SystemExit):
            sign_artifacts._load_signer("1122")


class WriteSignatureTests(unittest.TestCase):
    def test_record_shape_and_verifiability(self):
        signer = sign_artifacts._load_signer(TEST_SEED_HEX)
        data = b"artifact-bytes-v1"
        with tempfile.TemporaryDirectory() as tmp:
            version_dir = write_version(Path(tmp), "core.example", "1.0.0")
            out = sign_artifacts._write_signature(version_dir, signer, data)
            record = json.loads(out.read_text())
            self.assertEqual(set(record), {"scheme", "public_key_hex", "signature_hex", "sigstore_bundle_ref", "signed_at"})
            self.assertEqual(record["scheme"], "ed25519")
            self.assertIsNone(record["sigstore_bundle_ref"])
            self.assertTrue(record["signed_at"].endswith("Z"))
            pub = Ed25519PublicKey.from_public_bytes(bytes.fromhex(record["public_key_hex"]))
            pub.verify(bytes.fromhex(record["signature_hex"]), data)


class NeedsSignatureTests(unittest.TestCase):
    def _needs(self, **kw):
        with tempfile.TemporaryDirectory() as tmp:
            cwd = os.getcwd()
            try:
                os.chdir(tmp)
                version_dir = write_version(Path(tmp), "core.example", "1.0.0", **kw)
                return sign_artifacts._needs_signature(version_dir / "contract.json")
            finally:
                os.chdir(cwd)

    def test_plain_artifact_bearing_version_needs_signature(self):
        self.assertTrue(self._needs())

    def test_deprecated_version_is_skipped(self):
        self.assertFalse(self._needs(deprecated=True))

    def test_workflow_backed_version_is_skipped(self):
        self.assertFalse(self._needs(artifact=False))

    def test_already_signed_version_is_skipped(self):
        self.assertFalse(self._needs(signed=True))


class MainTests(unittest.TestCase):
    def _run_main(self, argv, env_key=None):
        with tempfile.TemporaryDirectory() as tmp:
            cwd = os.getcwd()
            old_argv = sys.argv
            had_key = os.environ.pop(sign_artifacts.SECRET_KEY_ENV, None)
            try:
                os.chdir(tmp)
                write_version(Path(tmp), "core.alpha", "1.0.0")
                write_version(Path(tmp), "core.beta", "1.0.0", deprecated=True)
                write_version(Path(tmp), "core.gamma", "1.0.0", artifact=False)
                if env_key is not None:
                    os.environ[sign_artifacts.SECRET_KEY_ENV] = env_key
                sys.argv = ["sign_artifacts.py", *argv]
                rc = sign_artifacts.main()
                signed_exists = (Path(tmp) / "capabilities/core/core.alpha/1.0.0/signature.json").is_file()
                return rc, signed_exists
            finally:
                os.chdir(cwd)
                sys.argv = old_argv
                os.environ.pop(sign_artifacts.SECRET_KEY_ENV, None)
                if had_key is not None:
                    os.environ[sign_artifacts.SECRET_KEY_ENV] = had_key

    def test_dry_run_writes_nothing(self):
        rc, signed = self._run_main(["--all", "--dry-run"])
        self.assertEqual(rc, 0)
        self.assertFalse(signed)

    def test_missing_key_exits_zero_and_writes_nothing(self):
        rc, signed = self._run_main(["--all"])
        self.assertEqual(rc, 0)
        self.assertFalse(signed)

    def test_requires_a_mode(self):
        with self.assertRaises(SystemExit):
            self._run_main([])


if __name__ == "__main__":
    unittest.main()
