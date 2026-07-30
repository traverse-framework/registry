#!/usr/bin/env python3
"""Build the versioned aggregated index artifact.

Implements specs/003-index-release-pipeline/spec.md FR-001, FR-004, and the
deprecation reflection required by specs/005-yank-deprecation/spec.md FR-003.
Also implements specs/009-contract-metadata-in-index/spec.md (Draft) FR-001
through FR-004: contract provenance fields (`contract_digest`,
`contract_url`) per entry, and hard-failing the build on an unreadable
contract.json instead of silently omitting it from the index.

Artifact-reference handling (added after a real incident: traverse-cli's
`capability publish` silently drops `artifact`/`artifact_type` on every
publish -- see registry#89/#90/#92 and traverse-framework/traverse#859 --
producing `digest: null`/`artifact_url: null` entries that crashed
`registry sync` for every consumer, since the whole index fails to
deserialize once it hits one null record):
  - An ACTIVE (non-deprecated) contract missing `artifact.digest`/`.url`
    hard-fails the build, same failure class as an unreadable contract --
    an unusable record must never reach a consumer.
  - A DEPRECATED contract missing them is excluded from the index
    entirely (not included with null fields) rather than failing the
    build. Contracts are immutable, so an already-broken deprecated
    version can never be fixed by editing; excluding it is the only way
    a future index build can ever succeed again. Its `contract.json` and
    `deprecated.json` remain in git history untouched -- only the
    aggregate index's *inclusion* of it changes, per spec 005's yank
    mechanism already treating index presence as additive, not something
    the underlying files control directly.

Also implements specs/001-registry-foundation/spec.md FR-013 (decision-log
entry 44): a `workflows[]` array, built the same way `capabilities[]` is.
A workflow.json has no separate compiled artifact -- the JSON file itself is
the whole published record -- so each entry gets a `workflow_digest`/
`workflow_url` pair (the same provenance purpose as `contract_digest`/
`contract_url` above) instead of an `artifact`-style digest/url pair.
`workflows/examples/` is excluded -- it holds pre-FR-013 demo/fixture
content that doesn't follow the real `workflows/<namespace>/<id>/<version>/`
layout, the same way this script never walks `examples/applications/`.

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
            artifact_digest = artifact.get("digest")
            artifact_url = artifact.get("url")

            if not artifact_digest or not artifact_url:
                if deprecated:
                    # Permanently broken and unfixable (contracts are
                    # immutable) -- omit from the index rather than emit
                    # null fields that would crash every consumer's parse.
                    continue
                raise IndexBuildError(
                    "index.missing_artifact_reference",
                    str(contract_path),
                    "active contract has no artifact.digest/artifact.url -- "
                    "an unusable record must not reach a consumer",
                )

            contract_digest = f"sha256:{hashlib.sha256(raw_bytes).hexdigest()}"
            contract_url = f"https://raw.githubusercontent.com/{repo_slug}/{source_commit}/{contract_path.as_posix()}"

            entries.append(
                {
                    "namespace": contract.get("namespace"),
                    "id": contract.get("id"),
                    "version": contract.get("version"),
                    "digest": artifact_digest,
                    "artifact_url": artifact_url,
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
        "workflows": build_workflow_entries(source_commit, repo_slug),
    }


def build_workflow_entries(source_commit: str, repo_slug: str) -> list:
    workflows_dir = Path("workflows")
    entries = []

    if not workflows_dir.is_dir():
        return entries

    for workflow_path in sorted(p for p in workflows_dir.rglob("workflow.json") if "examples" not in p.parts):
        try:
            raw_bytes = workflow_path.read_bytes()
            workflow = json.loads(raw_bytes)
        except Exception as exc:
            raise IndexBuildError(
                "index.workflow_unreadable",
                str(workflow_path),
                f"Unable to read/parse workflow.json: {exc}",
            )

        deprecated_path = workflow_path.parent / "deprecated.json"
        deprecated = deprecated_path.is_file()

        workflow_digest = f"sha256:{hashlib.sha256(raw_bytes).hexdigest()}"
        workflow_url = f"https://raw.githubusercontent.com/{repo_slug}/{source_commit}/{workflow_path.as_posix()}"

        entries.append(
            {
                "namespace": workflow.get("namespace"),
                "id": workflow.get("id"),
                "version": workflow.get("version"),
                "workflow_digest": workflow_digest,
                "workflow_url": workflow_url,
                "deprecated": deprecated,
            }
        )

    return entries


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
    print(
        f"Built index_version={index['index_version']} with {len(index['capabilities'])} capabilities "
        f"and {len(index['workflows'])} workflows at {output_path}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
