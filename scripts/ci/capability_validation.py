#!/usr/bin/env python3
"""Deterministic capability validation gate.

Implements specs/002-capability-validation/spec.md FR-001 through FR-005.
Walks capabilities/**/contract.json, validates schema/path/semver, checks
namespace-collision-safe immutability (no PR may modify an existing
contract.json), and checks yank records (specs/005-yank-deprecation)
never accompany a modified contract.json.
"""

import json
import re
import subprocess
import sys
from pathlib import Path

SEMVER_RE = re.compile(
    r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)"
    r"(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)"
    r"(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?"
    r"(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$"
)

REQUIRED_FIELDS = ["id", "namespace", "owner", "version"]


def fail(errors, code, path, message):
    errors.append({"code": code, "path": path, "message": message})


def validate_contract(path: Path, errors: list) -> None:
    try:
        contract = json.loads(path.read_text())
    except Exception as exc:
        fail(errors, "contract.invalid_json", str(path), f"Unable to parse JSON: {exc}")
        return

    for field in REQUIRED_FIELDS:
        if field not in contract:
            fail(
                errors,
                "contract.missing_required_field",
                str(path),
                f"Missing required field '{field}'",
            )

    # path is capabilities/<namespace>/<id>/<version>/contract.json
    parts = path.parts
    try:
        idx = parts.index("capabilities")
        namespace_seg, id_seg, version_seg = parts[idx + 1], parts[idx + 2], parts[idx + 3]
    except (ValueError, IndexError):
        fail(errors, "contract.bad_path", str(path), "Path does not match capabilities/<namespace>/<id>/<version>/contract.json")
        return

    if contract.get("namespace") and contract.get("namespace") != namespace_seg:
        fail(
            errors,
            "contract.namespace_mismatch",
            str(path),
            f"contract.json namespace '{contract.get('namespace')}' does not match path segment '{namespace_seg}'",
        )

    if contract.get("id") and contract.get("id") != id_seg:
        fail(
            errors,
            "contract.id_mismatch",
            str(path),
            f"contract.json id '{contract.get('id')}' does not match path segment '{id_seg}'",
        )

    version = contract.get("version")
    if version and version != version_seg:
        fail(
            errors,
            "contract.version_mismatch",
            str(path),
            f"contract.json version '{version}' does not match path segment '{version_seg}'",
        )

    if version and not SEMVER_RE.match(version):
        fail(errors, "contract.invalid_semver", str(path), f"'{version}' is not a valid semver string")

    artifact = contract.get("artifact")
    if artifact is not None:
        if not isinstance(artifact, dict) or "digest" not in artifact or "url" not in artifact:
            fail(
                errors,
                "contract.invalid_artifact_reference",
                str(path),
                "artifact reference must include 'digest' and 'url'",
            )
        elif not str(artifact["digest"]).startswith("sha256:"):
            fail(errors, "contract.invalid_digest_format", str(path), "artifact digest must be a 'sha256:' prefixed value")


def check_immutability(base_sha: str, head_sha: str, errors: list) -> None:
    """FR: no PR may modify an existing contract.json (specs/001 FR-002, specs/005 FR-002)."""
    diff = subprocess.check_output(
        ["git", "diff", "--name-status", f"{base_sha}...{head_sha}", "--", "capabilities/"],
        text=True,
    )
    for line in diff.splitlines():
        if not line.strip():
            continue
        parts = line.split("\t")
        status = parts[0]
        path = parts[-1]
        if path.endswith("contract.json") and status != "A":
            fail(
                errors,
                "capabilities.contract_modified",
                path,
                f"contract.json must never be modified once published (git status: {status}). "
                "Use a new version directory, or a deprecated.json sibling to yank.",
            )


def _semver_tuple(version: str):
    match = SEMVER_RE.match(version)
    if not match:
        return None
    return tuple(int(p) for p in version.split("+")[0].split("-")[0].split("."))


def classify_change(previous: dict, current: dict) -> str:
    """Returns 'major', 'minor', or 'patch' per specs/002-capability-validation.md."""
    prev_fields = set(previous.keys())
    curr_fields = set(current.keys())

    removed_required = [
        f for f in prev_fields - curr_fields if f in REQUIRED_FIELDS or f in previous.get("required", [])
    ]
    if removed_required:
        return "major"

    for field in prev_fields & curr_fields:
        if field in ("description",):
            continue
        if previous[field] != current[field]:
            if field in ("input_schema", "output_schema", "events", "permissions", "constraints"):
                return "major"

    if curr_fields - prev_fields:
        return "minor"

    return "patch"


def check_semver_bump(errors: list) -> None:
    """FR-002: declared bump must be >= detected change class vs. the prior version."""
    capabilities_dir = Path("capabilities")
    if not capabilities_dir.is_dir():
        return

    by_namespace_id: dict = {}
    for contract_path in sorted(capabilities_dir.rglob("contract.json")):
        parts = contract_path.parts
        try:
            idx = parts.index("capabilities")
            namespace_seg, id_seg, version_seg = parts[idx + 1], parts[idx + 2], parts[idx + 3]
        except (ValueError, IndexError):
            continue
        by_namespace_id.setdefault((namespace_seg, id_seg), []).append((version_seg, contract_path))

    for (namespace_seg, id_seg), versions in by_namespace_id.items():
        parsed = [(v, p, _semver_tuple(v)) for v, p in versions]
        parsed = [t for t in parsed if t[2] is not None]
        parsed.sort(key=lambda t: t[2])
        for i in range(1, len(parsed)):
            prev_version, prev_path, prev_tuple = parsed[i - 1]
            curr_version, curr_path, curr_tuple = parsed[i]
            try:
                previous = json.loads(prev_path.read_text())
                current = json.loads(curr_path.read_text())
            except Exception:
                continue
            change_class = classify_change(previous, current)
            bump = "major" if curr_tuple[0] > prev_tuple[0] else "minor" if curr_tuple[1] > prev_tuple[1] else "patch"
            rank = {"patch": 0, "minor": 1, "major": 2}
            if rank[bump] < rank[change_class]:
                fail(
                    errors,
                    "semver.bump_too_small",
                    str(curr_path),
                    f"Detected a '{change_class}' change from {prev_version} but version bump was only '{bump}'",
                )


def check_dependency_resolvability(errors: list) -> None:
    """FR-005: declared dependencies must resolve against already-published capabilities."""
    capabilities_dir = Path("capabilities")
    if not capabilities_dir.is_dir():
        return

    published: dict = {}
    for contract_path in sorted(capabilities_dir.rglob("contract.json")):
        try:
            contract = json.loads(contract_path.read_text())
        except Exception:
            continue
        key = (contract.get("namespace"), contract.get("id"))
        published.setdefault(key, []).append(contract.get("version"))

    for contract_path in sorted(capabilities_dir.rglob("contract.json")):
        try:
            contract = json.loads(contract_path.read_text())
        except Exception:
            continue
        for dep in contract.get("dependencies", []) or []:
            dep_id = dep.get("capability_id")
            dep_range = dep.get("version_range")
            if not dep_id:
                continue
            # capability_id may be "namespace/id" or just "id" (defaults to core namespace)
            if "/" in dep_id:
                dep_namespace, dep_short_id = dep_id.split("/", 1)
            else:
                dep_namespace, dep_short_id = "core", dep_id
            versions = published.get((dep_namespace, dep_short_id))
            if not versions:
                fail(
                    errors,
                    "dependency_unsatisfiable",
                    str(contract_path),
                    f"Dependency '{dep_id}' ({dep_range}) does not resolve to any published capability in this registry",
                )


def main() -> int:
    errors: list = []
    capabilities_dir = Path("capabilities")

    if capabilities_dir.is_dir():
        for contract_path in sorted(capabilities_dir.rglob("contract.json")):
            validate_contract(contract_path, errors)
        check_semver_bump(errors)
        check_dependency_resolvability(errors)

    base_sha = None
    head_sha = None
    if len(sys.argv) >= 3:
        base_sha, head_sha = sys.argv[1], sys.argv[2]
    if base_sha and head_sha:
        try:
            check_immutability(base_sha, head_sha, errors)
        except subprocess.CalledProcessError as exc:
            fail(errors, "git.diff_failed", "capabilities/", f"Unable to compute diff: {exc}")

    status = "passed" if not errors else "failed"
    print(json.dumps({"status": status, "failures": errors}, indent=2))

    if errors:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
