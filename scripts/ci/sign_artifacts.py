#!/usr/bin/env python3
"""Ed25519-sign published capability artifacts.

Implements the signing half of `specs/007-artifact-hosting`'s amendment
(registry#331 / registry#333, `docs/decision-log.md` entries 74/75). Digest
verification (spec 007 FR-001/FR-002) already proves an artifact's *integrity*;
`traverse-runtime`'s execution-time trust model additionally requires proof of
*authenticity* -- a real cryptographic signature -- before it will run any
capability classified `ArtifactTrustLevel::PublishedGoverned`. This script
produces that signature as an additive `signature.json` sibling next to each
non-deprecated, artifact-bearing `contract.json`, and never touches the
immutable `contract.json` itself (spec 007 Amendment FR-007/FR-012).

Modes (exactly one):

  --since-merge   Sign only capability versions whose `contract.json` was added
                  by the push currently being built -- the range
                  $BEFORE_SHA..$AFTER_SHA, falling back to HEAD~1..HEAD. This is
                  what the merge-to-`main` CI job runs (spec 007 Amendment
                  FR-009): signing is gated on the same human-approval-then-merge
                  boundary this repo already trusts, never on PR open/sync, and
                  no individual publisher ever holds the key.

  --all           Sign every non-deprecated, artifact-bearing version that has
                  no `signature.json` yet. This is the one-time backfill
                  (registry#335) and doubles as a self-healing safety net.

  --dry-run       Print what would be signed and exit; write nothing, fetch
                  nothing, and require no key. Combine with a mode above.

The Ed25519 private key is read from $ARTIFACT_SIGNING_ED25519_SECRET_KEY as 64
hex characters (a 32-byte seed). It is never written to stdout/stderr or into
any file. When the variable is unset the script exits 0 after printing a notice
(the CI job stays inert until the repo owner provisions the secret -- same
"code ready, owner setting pending" posture as `deploy-catalog`).
"""

import argparse
import json
import os
import subprocess
import sys
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

CAPABILITIES_DIR = Path("capabilities")
PUBLIC_KEY_PATH = Path("catalog") / "signing-key.pub"
SECRET_KEY_ENV = "ARTIFACT_SIGNING_ED25519_SECRET_KEY"
SIGNATURE_SCHEME = "ed25519"
_ZERO_SHA = "0000000000000000000000000000000000000000"


def _load_signer(seed_hex: str):
    """Return an Ed25519PrivateKey from a 32-byte hex seed. Imported lazily so
    --dry-run and the "no key configured" path need no third-party dependency."""
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

    seed_hex = seed_hex.strip()
    try:
        seed = bytes.fromhex(seed_hex)
    except ValueError as exc:
        raise SystemExit(f"{SECRET_KEY_ENV} is not valid hex: {exc}")
    if len(seed) != 32:
        raise SystemExit(
            f"{SECRET_KEY_ENV} must be 64 hex chars (32-byte Ed25519 seed); got {len(seed)} bytes"
        )
    return Ed25519PrivateKey.from_private_bytes(seed)


def _public_key_hex(signer) -> str:
    from cryptography.hazmat.primitives import serialization

    raw = signer.public_key().public_bytes(
        encoding=serialization.Encoding.Raw,
        format=serialization.PublicFormat.Raw,
    )
    return raw.hex()


def _sign_hex(signer, data: bytes) -> str:
    return signer.sign(data).hex()


def _is_deprecated(version_dir: Path) -> bool:
    return (version_dir / "deprecated.json").is_file()


def _artifact_url(contract_path: Path):
    try:
        contract = json.loads(contract_path.read_text())
    except (OSError, json.JSONDecodeError):
        return None
    artifact = contract.get("artifact")
    if not isinstance(artifact, dict):
        return None
    url = artifact.get("url")
    return url if isinstance(url, str) and url else None


def _download(url: str, attempts: int = 4) -> bytes:
    last_exc = None
    for i in range(attempts):
        try:
            with urllib.request.urlopen(url, timeout=60) as resp:  # noqa: S310 (fixed github.com host)
                return resp.read()
        except Exception as exc:  # noqa: BLE001 - retried, then re-raised
            last_exc = exc
            time.sleep(2 * (i + 1))
    raise RuntimeError(f"failed to download {url}: {last_exc}")


def _added_contract_paths(before_sha: str, after_sha: str) -> list:
    rng = f"{before_sha}..{after_sha}"
    out = subprocess.check_output(
        ["git", "diff", "--name-status", "--diff-filter=A", rng, "--", "capabilities/"],
        text=True,
    )
    paths = []
    for line in out.splitlines():
        if not line.strip():
            continue
        parts = line.split("\t")
        path = parts[-1]
        if path.endswith("/contract.json"):
            paths.append(Path(path))
    return paths


def _merge_range() -> tuple:
    before = os.environ.get("BEFORE_SHA", "").strip()
    after = os.environ.get("AFTER_SHA", "").strip() or "HEAD"
    if not before or before == _ZERO_SHA:
        before = "HEAD~1"
    return before, after


def _needs_signature(contract_path: Path) -> bool:
    version_dir = contract_path.parent
    if _is_deprecated(version_dir):
        return False
    if _artifact_url(contract_path) is None:
        return False
    return not (version_dir / "signature.json").is_file()


def _targets(mode: str) -> list:
    if mode == "all":
        candidates = sorted(CAPABILITIES_DIR.rglob("contract.json"))
    else:
        before, after = _merge_range()
        candidates = _added_contract_paths(before, after)
    return [p for p in candidates if _needs_signature(p)]


def _write_signature(version_dir: Path, signer, data: bytes) -> Path:
    record = {
        "scheme": SIGNATURE_SCHEME,
        "public_key_hex": _public_key_hex(signer),
        "signature_hex": _sign_hex(signer, data),
        "sigstore_bundle_ref": None,
        "signed_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    }
    out = version_dir / "signature.json"
    out.write_text(json.dumps(record, indent=2) + "\n")
    return out


def _refresh_public_key(signer) -> bool:
    """Keep catalog/signing-key.pub current so deploy-catalog publishes it at
    the well-known URL (spec 007 Amendment FR-010). Returns True if it changed."""
    PUBLIC_KEY_PATH.parent.mkdir(parents=True, exist_ok=True)
    want = _public_key_hex(signer) + "\n"
    if PUBLIC_KEY_PATH.is_file() and PUBLIC_KEY_PATH.read_text() == want:
        return False
    PUBLIC_KEY_PATH.write_text(want)
    return True


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--since-merge", action="store_true", help="sign contracts added by the current push")
    group.add_argument("--all", action="store_true", help="backfill: sign every unsigned non-deprecated version")
    parser.add_argument("--dry-run", action="store_true", help="report only; write and fetch nothing")
    args = parser.parse_args()

    if not CAPABILITIES_DIR.is_dir():
        print(json.dumps({"status": "skipped", "reason": "no capabilities/ directory"}))
        return 0

    mode = "all" if args.all else "since-merge"
    targets = _targets(mode)

    if args.dry_run:
        print(json.dumps({"status": "dry-run", "mode": mode, "would_sign": [str(p.parent) for p in targets]}, indent=2))
        return 0

    seed_hex = os.environ.get(SECRET_KEY_ENV)
    if not seed_hex:
        print(json.dumps({
            "status": "skipped",
            "reason": f"{SECRET_KEY_ENV} not set -- signing key not yet provisioned",
            "pending_targets": [str(p.parent) for p in targets],
        }, indent=2))
        return 0

    signer = _load_signer(seed_hex)
    key_changed = _refresh_public_key(signer)

    signed = []
    failures = []
    for contract_path in targets:
        version_dir = contract_path.parent
        url = _artifact_url(contract_path)
        try:
            data = _download(url)
            out = _write_signature(version_dir, signer, data)
            signed.append(str(out))
        except Exception as exc:  # noqa: BLE001 - reported, non-zero exit below
            failures.append({"path": str(version_dir), "error": str(exc)})

    print(json.dumps({
        "status": "failed" if failures else "ok",
        "mode": mode,
        "signed": signed,
        "public_key_refreshed": key_changed,
        "failures": failures,
    }, indent=2))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
