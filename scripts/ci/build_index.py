#!/usr/bin/env python3
"""Build the versioned aggregated index artifact.

Implements specs/003-index-release-pipeline/spec.md FR-001, FR-004, and the
deprecation reflection required by specs/005-yank-deprecation/spec.md FR-003.

Usage: build_index.py <previous_index_version_or_0> <source_commit_sha> <output_path>
"""

import json
import sys
from datetime import datetime, timezone
from pathlib import Path


def build_index(previous_index_version: int, source_commit: str) -> dict:
    capabilities_dir = Path("capabilities")
    entries = []

    if capabilities_dir.is_dir():
        for contract_path in sorted(capabilities_dir.rglob("contract.json")):
            try:
                contract = json.loads(contract_path.read_text())
            except Exception:
                continue

            deprecated_path = contract_path.parent / "deprecated.json"
            deprecated = deprecated_path.is_file()

            artifact = contract.get("artifact") or {}
            entries.append(
                {
                    "namespace": contract.get("namespace"),
                    "id": contract.get("id"),
                    "version": contract.get("version"),
                    "digest": artifact.get("digest"),
                    "artifact_url": artifact.get("url"),
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
    if len(sys.argv) != 4:
        print("Usage: build_index.py <previous_index_version_or_0> <source_commit_sha> <output_path>", file=sys.stderr)
        return 1

    previous_index_version = int(sys.argv[1])
    source_commit = sys.argv[2]
    output_path = Path(sys.argv[3])

    index = build_index(previous_index_version, source_commit)
    output_path.write_text(json.dumps(index, indent=2) + "\n")
    print(f"Built index_version={index['index_version']} with {len(index['capabilities'])} capabilities at {output_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
