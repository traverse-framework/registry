#!/usr/bin/env python3
"""Opt-in publication-lifecycle measurement export helper.

specs/022-publication-lifecycle-measurement FR-014/FR-015 (registry#354,
decision-log entry 83).

Builds a candidate `metrics/publication-lifecycle/pr-<n>.json` record for a
merged or closed capability PR against traverse-framework/registry:

  * period / outcome / hours_open_to_conclusion / review_rounds come from
    `gh pr view` metadata;
  * the complexity rubric and its verdict come from the PR's added
    contract.json and its changed-file list, read locally at the PR's merge
    commit.

It is opt-in by construction: nothing runs it automatically. It only writes
the file -- you review it and open a normal PR to contribute it. Aggregate,
redacted, no identifiers: see the record schema and
scripts/ci/measurement_validation.py for what is and isn't allowed.

Usage:
    scripts/metrics/publication_lifecycle_export.py --pr 358
    scripts/metrics/publication_lifecycle_export.py --pr 358 \\
        --set-complexity complex --reason "small schema, subtle state machine"

Requires `gh` authenticated against traverse-framework/registry and a local
checkout whose history contains the PR's merge commit (git fetch origin main).
"""

from __future__ import annotations

import argparse
import json
import math
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

REPO = "traverse-framework/registry"
RECORDS_DIR = Path("metrics/publication-lifecycle")
DEFAULT_SCHEMA_FIELD_THRESHOLD = 20
RECORD_SCHEMA_VERSION = "1.0.0"


# --------------------------------------------------------------------------
# Pure helpers (unit-tested in scripts/metrics/tests/)
# --------------------------------------------------------------------------
def count_schema_properties(node) -> int:
    """Total number of `properties` keys, counted recursively."""
    total = 0
    if isinstance(node, dict):
        props = node.get("properties")
        if isinstance(props, dict):
            total += len(props)
        for value in node.values():
            total += count_schema_properties(value)
    elif isinstance(node, list):
        for item in node:
            total += count_schema_properties(item)
    return total


def compute_schema_field_count(contract: dict) -> int:
    inputs = contract.get("inputs", {}) or {}
    outputs = contract.get("outputs", {}) or {}
    return count_schema_properties(inputs.get("schema")) + count_schema_properties(
        outputs.get("schema")
    )


def compute_effectful(contract: dict) -> bool:
    """FR-008(1): not pure/deterministic."""
    side_effects = contract.get("side_effects") or []
    if any((se or {}).get("kind") != "memory_only" for se in side_effects):
        return True
    constraints = ((contract.get("execution") or {}).get("constraints")) or {}
    if constraints.get("host_api_access", "none") != "none":
        return True
    if constraints.get("filesystem_access", "none") != "none":
        return True
    if constraints.get("network_access", "forbidden") != "forbidden":
        return True
    for field in ("connector_requirements", "emits", "consumes"):
        if contract.get(field):
            return True
    return False


def build_rubric(contract: dict, changed_paths, threshold: int) -> dict:
    changed_paths = list(changed_paths)
    changed_specs = any(p.startswith("specs/") for p in changed_paths)
    added_persona = any(
        p.startswith("personas/") and p.endswith("/persona.json") for p in changed_paths
    )
    return {
        "effectful": compute_effectful(contract),
        "dependency_count": len(contract.get("dependencies") or []),
        "changed_specs": changed_specs,
        "added_persona": added_persona,
        "schema_field_count": compute_schema_field_count(contract),
        "schema_field_threshold": threshold,
    }


def rubric_verdict(rubric: dict) -> str:
    simple = (
        rubric["effectful"] is False
        and rubric["dependency_count"] == 0
        and rubric["changed_specs"] is False
        and rubric["added_persona"] is False
        and rubric["schema_field_count"] <= rubric["schema_field_threshold"]
    )
    return "simple" if simple else "complex"


def _parse_iso(ts: str) -> datetime:
    return datetime.fromisoformat(ts.replace("Z", "+00:00")).astimezone(timezone.utc)


def derive_period(conclusion_ts: str) -> str:
    dt = _parse_iso(conclusion_ts)
    return f"{dt.year:04d}-{dt.month:02d}"


def hours_between(start_ts: str, end_ts: str) -> int:
    delta = _parse_iso(end_ts) - _parse_iso(start_ts)
    return max(0, math.floor(delta.total_seconds() / 3600))


def count_review_rounds(reviews) -> int:
    """Distinct submitted reviews (any non-pending state)."""
    return sum(
        1 for r in (reviews or []) if (r or {}).get("state", "").upper() != "PENDING"
    )


def build_record(
    pr_number: int,
    pr_meta: dict,
    contract: dict,
    changed_paths,
    threshold: int,
    override_final: str | None,
    override_reason: str | None,
) -> dict:
    merged = bool(pr_meta.get("mergedAt"))
    outcome = "merged" if merged else "closed_unmerged"
    conclusion_ts = pr_meta.get("mergedAt") or pr_meta.get("closedAt")
    if not conclusion_ts:
        raise SystemExit(f"PR #{pr_number} is neither merged nor closed; nothing to measure.")

    rubric = build_rubric(contract, changed_paths, threshold)
    computed = rubric_verdict(rubric)

    if override_final:
        if override_final not in ("simple", "complex"):
            raise SystemExit("--set-complexity must be 'simple' or 'complex'.")
        if override_final == computed:
            raise SystemExit(
                f"--set-complexity {override_final} matches the computed value; "
                f"an override must change it."
            )
        if not override_reason or not override_reason.strip():
            raise SystemExit("--reason is required with --set-complexity.")
        reason = override_reason.strip()
        if len(reason) > 200:
            raise SystemExit("--reason must be at most 200 characters.")
        complexity = {
            "computed": computed,
            "final": override_final,
            "override": True,
            "override_reason": reason,
        }
    else:
        complexity = {
            "computed": computed,
            "final": computed,
            "override": False,
            "override_reason": None,
        }

    return {
        "record_schema_version": RECORD_SCHEMA_VERSION,
        "pr_number": pr_number,
        "period": derive_period(conclusion_ts),
        "outcome": outcome,
        "hours_open_to_conclusion": hours_between(pr_meta["createdAt"], conclusion_ts),
        "review_rounds": count_review_rounds(pr_meta.get("reviews")),
        "complexity": complexity,
        "rubric": rubric,
        "ci_failure_counts": {},
    }


# --------------------------------------------------------------------------
# gh / git plumbing
# --------------------------------------------------------------------------
def _gh_json(args) -> dict:
    out = subprocess.check_output(["gh", *args], text=True)
    return json.loads(out)


def fetch_pr_meta(pr_number: int) -> dict:
    return _gh_json(
        [
            "pr",
            "view",
            str(pr_number),
            "--repo",
            REPO,
            "--json",
            "number,createdAt,mergedAt,closedAt,mergeCommit,files,reviews",
        ]
    )


def added_contract_path(pr_meta: dict) -> str:
    files = [f["path"] for f in pr_meta.get("files", [])]
    contracts = [
        p
        for p in files
        if p.startswith("capabilities/") and p.endswith("/contract.json")
    ]
    if not contracts:
        raise SystemExit(
            "PR touched no capabilities/**/contract.json -- this helper measures "
            "capability publication PRs."
        )
    if len(contracts) > 1:
        raise SystemExit(
            f"PR adds multiple contracts ({contracts}); measure one publication per record."
        )
    return contracts[0]


def read_contract_at_commit(commit: str, path: str) -> dict:
    try:
        blob = subprocess.check_output(["git", "show", f"{commit}:{path}"], text=True)
    except subprocess.CalledProcessError as exc:
        raise SystemExit(
            f"Could not read {path} at {commit[:12]} -- run `git fetch origin main` "
            f"so the merge commit is local. ({exc})"
        )
    return json.loads(blob)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pr", type=int, required=True, help="PR number to measure.")
    parser.add_argument(
        "--set-complexity",
        choices=["simple", "complex"],
        help="Override the computed complexity bucket (records the override).",
    )
    parser.add_argument("--reason", help="Required with --set-complexity; <=200 chars.")
    parser.add_argument(
        "--threshold",
        type=int,
        default=DEFAULT_SCHEMA_FIELD_THRESHOLD,
        help=f"schema_field_count threshold (default {DEFAULT_SCHEMA_FIELD_THRESHOLD}).",
    )
    parser.add_argument(
        "--out-dir",
        default=str(RECORDS_DIR),
        help="Directory to write the record into.",
    )
    args = parser.parse_args(argv)

    pr_meta = fetch_pr_meta(args.pr)
    contract_path = added_contract_path(pr_meta)
    merge_commit = (pr_meta.get("mergeCommit") or {}).get("oid")
    ref = merge_commit or "HEAD"
    contract = read_contract_at_commit(ref, contract_path)
    changed_paths = [f["path"] for f in pr_meta.get("files", [])]

    record = build_record(
        args.pr,
        pr_meta,
        contract,
        changed_paths,
        args.threshold,
        args.set_complexity,
        args.reason,
    )

    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    out_path = out_dir / f"pr-{args.pr}.json"
    out_path.write_text(json.dumps(record, indent=2, sort_keys=True) + "\n")
    print(f"Wrote {out_path}")
    print(
        "Review it, then open a PR adding it. It will be checked by "
        "scripts/ci/measurement_validation.py."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
