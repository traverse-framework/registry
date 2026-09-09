#!/usr/bin/env python3
"""Mirror the browser-facing verified-retrieval surface for published
capabilities into the discovery catalog's GitHub Pages output
(registry#304, registry#383).

A browser consumer at an arbitrary origin running
`traverse-embedder-web`'s `registryCache` path (spec
080-embedded-registry-cache) needs to fetch AND digest-verify two things
per capability before handing the bytes to `BundleEmbedder`: the compiled
WASM artifact, and the behavioral contract.

1. WASM artifact, CORS-open. GitHub Release asset downloads (this repo's
   own artifacts/<id>-<version> releases, spec 007-artifact-hosting)
   redirect to a signed Azure Blob URL with no access-control-allow-origin
   header -- a browser that discovered a capability via catalog.json (which
   IS CORS-enabled, same Pages site) cannot then fetch the artifact bytes
   catalog.json points at. Confirmed by direct curl against both endpoints;
   see registry#304. This script re-hosts a read-only copy under the same
   Pages site at the exact path suffix its own artifact.url already uses
   after ".../releases/download/" (artifacts/<id>-<version>/<asset-name>) --
   so the CORS-enabled mirror URL is always a fixed prefix swap of the
   authoritative artifact.url, not a new piece of state to keep in sync
   (generate_catalog_pages.py derives it the same way when rendering the
   link). Mirrored bytes are re-verified against artifact.digest before
   being written; a mismatch fails CI rather than silently serving
   corrupted bytes.

2. Contract, CORS-open, with a registry-authored digest (registry#383).
   catalog.json carries the contract INLINE, but a browser cannot treat
   the website's re-serialization of it as a verified artifact -- the
   digest has to be over bytes the registry authored. So this script also
   copies each version's immutable
   capabilities/<namespace>/<id>/<version>/contract.json *verbatim* next to
   its WASM (artifacts/<namespace>.<id>-<version>/contract.json), writes a
   sibling artifacts/<namespace>.<id>-<version>/contract.json.sha256
   holding "sha256:<hex>" of those exact bytes, and adds `contract_url` +
   `contract_digest` to that capability's catalog.json entry. The digest
   is computed over the identical bytes with the identical
   "sha256:" + sha256(raw_bytes) recipe scripts/ci/build_index.py already
   uses for index.json's `contract_digest` (spec
   009-contract-metadata-in-index) -- so the two public surfaces agree
   byte for byte for the same version.

contract.json's artifact.digest/url remain the sole authoritative record
per spec 007 -- this mirror is a convenience read-path, never referenced by
a contract, so it carries none of spec 007's immutability obligations. It
is regenerated fresh on every catalog build, like catalog.json itself, and
walks the whole capabilities/ tree (deprecated versions included, same as
gather_catalog_data.py) since a yanked version's artifact and contract
must stay fetchable and verifiable too.

Usage: mirror_artifacts.py <catalog_output_dir> [site_base_url]
       site_base_url defaults to https://registry.traverse-framework.com
       (the Pages origin catalog-builder / generate_catalog_pages.py target).
"""

import hashlib
import json
import re
import sys
import urllib.request
from pathlib import Path
from typing import Optional

ROOT = Path(__file__).resolve().parents[2]

# The Pages origin the catalog is served from -- same value CI passes to
# generate_catalog_pages.py. Overridable via argv[2] for tests / previews.
DEFAULT_SITE_BASE_URL = "https://registry.traverse-framework.com"

# Kept in exact lockstep with capability_validation.py's ARTIFACT_RELEASE_URL_RE
# -- both encode the same spec 007 tag scheme.
ARTIFACT_URL_PREFIX = "https://github.com/traverse-framework/registry/releases/download/"
ARTIFACT_URL_RE = re.compile(r"^" + re.escape(ARTIFACT_URL_PREFIX) + r"(artifacts/[^/]+/[^/]+)$")


def mirror_relpath_for_url(url: str) -> Optional[str]:
    """The path under the catalog output dir a mirrored artifact lives at,
    or None if `url` isn't a recognized this-repo release-asset URL."""
    match = ARTIFACT_URL_RE.match(url)
    return match.group(1) if match else None


def contract_digest_for_bytes(raw: bytes) -> str:
    """"sha256:<hex>" of the exact contract.json bytes -- identical recipe to
    scripts/ci/build_index.py so catalog.json and index.json never disagree
    on a version's contract_digest."""
    return f"sha256:{hashlib.sha256(raw).hexdigest()}"


def inject_contract_provenance(catalog_path: Path, provenance_by_ref: dict) -> int:
    """Add `contract_url` / `contract_digest` to each capabilities[] entry in
    catalog.json whose `reference` we mirrored a contract for. Additive: no
    existing field is touched, entries with no mirrored contract are left as
    they were. Returns the number of entries updated.

    Tolerates a missing catalog.json (unit tests exercise the mirror step in
    isolation); in CI catalog-builder always writes it before this runs."""
    if not catalog_path.is_file():
        print(
            f"note: {catalog_path} not present -- skipping catalog.json contract-provenance injection",
            file=sys.stderr,
        )
        return 0

    catalog = json.loads(catalog_path.read_text())
    updated = 0
    for entry in catalog.get("capabilities") or []:
        provenance = provenance_by_ref.get(entry.get("reference"))
        if provenance is None:
            continue
        entry["contract_url"] = provenance["contract_url"]
        entry["contract_digest"] = provenance["contract_digest"]
        updated += 1

    catalog_path.write_text(json.dumps(catalog, indent=2) + "\n")
    return updated


def fetch(url: str) -> bytes:
    with urllib.request.urlopen(url, timeout=60) as response:  # noqa: S310 (fixed, validated host)
        return response.read()


def main(argv) -> int:
    if not 2 <= len(argv) <= 3:
        print("Usage: mirror_artifacts.py <catalog_output_dir> [site_base_url]", file=sys.stderr)
        return 2
    out_dir = Path(argv[1])
    site_base_url = (argv[2] if len(argv) == 3 else DEFAULT_SITE_BASE_URL).rstrip("/")

    seen_urls = set()
    mirrored = 0
    skipped = 0
    contracts_mirrored = 0
    provenance_by_ref: dict = {}

    for contract_path in sorted(ROOT.glob("capabilities/*/*/*/contract.json")):
        raw_contract = contract_path.read_bytes()
        contract = json.loads(raw_contract)
        artifact = contract.get("artifact") or {}
        url = artifact.get("url")
        digest = artifact.get("digest")
        if not url or not digest:
            continue

        relpath = mirror_relpath_for_url(url)
        if relpath is None:
            print(f"SKIP (unrecognized artifact URL host/shape): {url} ({contract_path})", file=sys.stderr)
            skipped += 1
            continue

        namespace = contract.get("namespace") or ""
        capability_id = contract.get("id") or ""
        version = contract.get("version") or ""

        # (2) Mirror the immutable contract verbatim + a registry-authored
        # digest sibling, and record the provenance pair for the catalog.json
        # injection below. Cheap, local, and independent of the WASM fetch --
        # so it runs for every recognized version, deduped or not.
        #
        # The contract mirror path is keyed on the capability's own
        # <namespace>.<id>-<version> identity -- NOT on artifact.url's release
        # tag. When a later version reuses an earlier version's artifact
        # release (byte-identical WASM, a legitimate pattern), that tag names
        # the earlier version, so an artifact-URL-derived path collides: every
        # reusing version overwrites one file and no per-version mirror is
        # written for the newer ones (registry#421). Identity-keyed paths give
        # each version its own contract mirror regardless.
        if namespace and capability_id and version:
            contract_dir_rel = Path("artifacts") / f"{capability_id}-{version}"
            contract_dir = out_dir / contract_dir_rel
            contract_digest = contract_digest_for_bytes(raw_contract)
            contract_dir.mkdir(parents=True, exist_ok=True)
            (contract_dir / "contract.json").write_bytes(raw_contract)
            (contract_dir / "contract.json.sha256").write_text(contract_digest + "\n")
            contracts_mirrored += 1

            reference = f"{namespace}/{capability_id}@{version}"
            provenance_by_ref[reference] = {
                "contract_url": f"{site_base_url}/{contract_dir_rel.as_posix()}/contract.json",
                "contract_digest": contract_digest,
            }
        else:
            print(
                f"SKIP contract mirror (contract.json lacks namespace/id/version): {contract_path}",
                file=sys.stderr,
            )

        # (1) Mirror the WASM bytes: dedup by URL, verify against
        # artifact.digest, skip if already present. Path stays a fixed
        # prefix-swap of artifact.url (spec 007 CORS mirror invariant).
        if url in seen_urls:
            continue
        seen_urls.add(url)

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

    updated = inject_contract_provenance(out_dir / "catalog.json", provenance_by_ref)

    print(
        f"Mirrored {mirrored} WASM artifact(s) and {contracts_mirrored} contract(s) into "
        f"{out_dir}/artifacts/ ({skipped} skipped); added contract provenance to "
        f"{updated} catalog.json entr{'y' if updated == 1 else 'ies'} (registry#304/#383)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
