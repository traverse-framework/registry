#!/usr/bin/env python3
"""Build the versioned aggregated index artifact.

Implements specs/003-index-release-pipeline/spec.md FR-001, FR-004, and the
deprecation reflection required by specs/005-yank-deprecation/spec.md FR-003.
Also implements specs/009-contract-metadata-in-index/spec.md (Draft) FR-001
through FR-004: contract provenance fields (`contract_digest`,
`contract_url`) per entry, and hard-failing the build on an unreadable
contract.json instead of silently omitting it from the index.

Usage: build_index.py <previous_index_version_or_0> <source_commit_sha> <output_path> [repo_slug]
"""

import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

DEFAULT_REPO_SLUG = "traverse-framework/registry"


class IndexBuildError(Exception):
    def __init__(self, code: str, path: str, message: str):
        super().__init__(f"{code}: {path}: {message}")
        self.code = code
        self.path = path
        self.message = message


def build_index(previous_index_version: int, source_commit: str, repo_slug: str = DEFAULT_REPO_SLUG) -> dict:
    capabilities_dir = Path("capabilities")
    entries = []

    if capabilities_dir.is_dir():
        for contract_path in sorted(capabilities_dir.rglob("contract.json")):
            try:
                raw_bytes = contract_path.read_bytes()
                contract = json.loads(raw_bytes)
            except Exception as exc:
                raise IndexBuildError(
                    "index.contract_unreadable",
                    str(contract_path),
                    f"Unable to read/parse contract.json: {exc}",
                )

            deprecated_path = contract_path.parent / "deprecated.json"
            deprecated = deprecated_path.is_file()

            artifact = contract.get("artifact") or {}
            contract_digest = f"sha256:{hashlib.sha256(raw_bytes).hexdigest()}"
            contract_url = f"https://raw.githubusercontent.com/{repo_slug}/{source_commit}/{contract_path.as_posix()}"

            entries.append(
                {
                    "namespace": contract.get("namespace"),
                    "id": contract.get("id"),
                    "version": contract.get("version"),
                    "digest": artifact.get("digest"),
                    "artifact_url": artifact.get("url"),
                    "contract_digest": contract_digest,
                    "contract_url": contract_url,
                    "deprecated": deprecated,
                }
            )

    return {
        "index_version": previous_index_version + 1,
        "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "source_commit": source_commit,
        "capabilities": entries,
    }


def main() -> int:
    if len(sys.argv) not in (4, 5):
        print(
            "Usage: build_index.py <previous_index_version_or_0> <source_commit_sha> <output_path> [repo_slug]",
            file=sys.stderr,
        )
        return 1

    previous_index_version = int(sys.argv[1])
    source_commit = sys.argv[2]
    output_path = Path(sys.argv[3])
    repo_slug = sys.argv[4] if len(sys.argv) == 5 else DEFAULT_REPO_SLUG

    try:
        index = build_index(previous_index_version, source_commit, repo_slug)
    except IndexBuildError as exc:
        print(f"{exc.code}: {exc.path}: {exc.message}", file=sys.stderr)
        return 1

    output_path.write_text(json.dumps(index, indent=2) + "\n")
    print(f"Built index_version={index['index_version']} with {len(index['capabilities'])} capabilities at {output_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
