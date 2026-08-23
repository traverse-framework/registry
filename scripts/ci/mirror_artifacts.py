#!/usr/bin/env python3
"""Mirror published capability WASM artifacts into the discovery catalog's
GitHub Pages output so they're fetchable with CORS (registry#304).

GitHub Release asset downloads (this repo's own artifacts/<id>-<version>
releases, spec 007-artifact-hosting) redirect to a signed Azure Blob URL
with no access-control-allow-origin header -- any browser-side consumer
that discovers a capability via catalog.json (which IS CORS-enabled, same
Pages site) cannot then fetch the artifact bytes catalog.json points at.
Confirmed by direct curl against both endpoints; see registry#304.

This script re-hosts a read-only copy of each artifact directly under the
same Pages site catalog-builder already writes to, at the exact path
suffix its own artifact.url already uses after ".../releases/download/"
(artifacts/<id>-<version>/<asset-name>) -- so the CORS-enabled mirror URL
is always a fixed prefix swap of the authoritative artifact.url, not a new
piece of state to keep in sync (generate_catalog_pages.py derives it the
same way when rendering the link). Mirrored bytes are re-verified against
artifact.digest before being written; a mismatch fails CI rather than
silently serving corrupted bytes.

contract.json's artifact.digest/url remain the sole authoritative record
per spec 007 -- this mirror is a convenience read-path, never referenced by
a contract, so it carries none of spec 007's immutability obligations. It
is regenerated fresh on every catalog build, like catalog.json itself, and
walks the whole capabilities/ tree (deprecated versions included, same as
gather_catalog_data.py) since a yanked version's artifact must stay
fetchable too.

Usage: mirror_artifacts.py <catalog_output_dir>
"""

import hashlib
import json
import re
import sys
import urllib.request
from pathlib import Path
from typing import Optional

ROOT = Path(__file__).resolve().parents[2]

# Kept in exact lockstep with capability_validation.py's ARTIFACT_RELEASE_URL_RE
# -- both encode the same spec 007 tag scheme.
ARTIFACT_URL_PREFIX = "https://github.com/traverse-framework/registry/releases/download/"
ARTIFACT_URL_RE = re.compile(r"^" + re.escape(ARTIFACT_URL_PREFIX) + r"(artifacts/[^/]+/[^/]+)$")


def mirror_relpath_for_url(url: str) -> Optional[str]:
    """The path under the catalog output dir a mirrored artifact lives at,
    or None if `url` isn't a recognized this-repo release-asset URL."""
    match = ARTIFACT_URL_RE.match(url)
    return match.group(1) if match else None


def fetch(url: str) -> bytes:
    with urllib.request.urlopen(url, timeout=60) as response:  # noqa: S310 (fixed, validated host)
        return response.read()


def main(argv) -> int:
    if len(argv) != 2:
        print("Usage: mirror_artifacts.py <catalog_output_dir>", file=sys.stderr)
        return 2
    out_dir = Path(argv[1])

    seen_urls = set()
    mirrored = 0
    skipped = 0
    for contract_path in sorted(ROOT.glob("capabilities/*/*/*/contract.json")):
        contract = json.loads(contract_path.read_text())
        artifact = contract.get("artifact") or {}
        url = artifact.get("url")
        digest = artifact.get("digest")
        if not url or not digest:
            continue
        if url in seen_urls:
            continue
        seen_urls.add(url)

        relpath = mirror_relpath_for_url(url)
        if relpath is None:
            print(f"SKIP (unrecognized artifact URL host/shape): {url} ({contract_path})", file=sys.stderr)
            skipped += 1
            continue

        dest = out_dir / relpath
        if dest.exists():
            continue

        body = fetch(url)
        actual_digest = f"sha256:{hashlib.sha256(body).hexdigest()}"
        if actual_digest != digest:
            print(
                f"FATAL: digest mismatch mirroring {url}\n"
                f"  contract digest:   {digest}\n"
                f"  downloaded digest: {actual_digest}\n"
                f"  ({contract_path})",
                file=sys.stderr,
            )
            return 1

        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(body)
        mirrored += 1

    print(f"Mirrored {mirrored} artifact(s) into {out_dir}/artifacts/ ({skipped} skipped) (registry#304)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
