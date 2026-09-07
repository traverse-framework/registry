#!/usr/bin/env python3
"""Deterministic publication-lifecycle measurement gate.

Implements specs/022-publication-lifecycle-measurement FR-001 through FR-013
(decision-log entry 83, registry#354).

Whole-tree run (no args): validates the shape and redaction of every
metrics/publication-lifecycle/pr-<n>.json record -- schema conformance
(FR-001/FR-004..FR-007), the FR-002 redaction prohibitions this repo's
schema cannot express (no capability id / namespace / owner / author, no
email, no @handle, no URL, no timestamp finer than a calendar month), the
FR-005 override consistency rules, and the FR-006 rule that a record may not
misreport its own rubric verdict.

Diff-based run (`measurement_validation.py <base_sha> <head_sha>`): also
fails if any already-merged pr-<n>.json record is modified or deleted in that
range -- records are immutable once merged, a correction is a fresh record
for a fresh PR (mirrors capability_validation.py's check_immutability).

No network, no gh, no cargo: reads only the record files and
`git diff --name-status`. Prints {"status": ..., "failures": [...]} and exits
non-zero on any failure, the same contract as capability_validation.py.
"""

import json
import re
import subprocess
import sys
from pathlib import Path

RECORDS_DIR = Path("metrics/publication-lifecycle")
RECORD_FILE_RE = re.compile(r"^pr-([1-9][0-9]*)\.json$")

RECORD_SCHEMA_VERSION = "1.0.0"

TOP_LEVEL_FIELDS = {
    "record_schema_version",
    "pr_number",
    "period",
    "outcome",
    "hours_open_to_conclusion",
    "review_rounds",
    "complexity",
    "rubric",
    "ci_failure_counts",
}
COMPLEXITY_FIELDS = {"computed", "final", "override", "override_reason"}
RUBRIC_FIELDS = {
    "effectful",
    "dependency_count",
    "changed_specs",
    "added_persona",
    "schema_field_count",
    "schema_field_threshold",
}
COMPLEXITY_VALUES = {"simple", "complex"}
OUTCOME_VALUES = {"merged", "closed_unmerged"}

# specs/022 FR-010 -- the stable CI-failure category set.
CI_FAILURE_CATEGORIES = {
    "schema",
    "semver",
    "digest",
    "namespace_collision",
    "dependency_resolvability",
    "immutability",
    "persona_ref",
    "use_cases_format",
    "use_cases_coverage",
    "action_enum_coverage",
    "artifact_reference",
    "test_coverage",
    "signature",
    "ecca_inventory",
    "event_product",
    "spec_alignment",
    "other",
}

PERIOD_RE = re.compile(r"^[0-9]{4}-(0[1-9]|1[0-2])$")

# specs/022 FR-002 -- identity-bearing keys forbidden at any depth.
FORBIDDEN_KEY_SUBSTRINGS = (
    "capability",
    "namespace",
    "owner",
    "team",
    "author",
    "publisher",
    "login",
    "email",
    "user",
)
EMAIL_RE = re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")
URL_RE = re.compile(r"https?://|www\.", re.IGNORECASE)
# specs/022 FR-003 -- any date (YYYY-MM-DD) or clock time is finer than the
# permitted calendar-month precision. `period` (YYYY-MM) does not match this.
FINE_TIMESTAMP_RE = re.compile(r"[0-9]{4}-[0-9]{2}-[0-9]{2}|[0-9]{2}:[0-9]{2}")


def fail(errors, code, path, message):
    errors.append({"code": code, "path": str(path), "message": message})


def rubric_verdict(rubric: dict) -> str:
    """specs/022 FR-008: `simple` iff all five conditions hold."""
    simple = (
        rubric.get("effectful") is False
        and rubric.get("dependency_count") == 0
        and rubric.get("changed_specs") is False
        and rubric.get("added_persona") is False
        and isinstance(rubric.get("schema_field_count"), int)
        and isinstance(rubric.get("schema_field_threshold"), int)
        and rubric["schema_field_count"] <= rubric["schema_field_threshold"]
    )
    return "simple" if simple else "complex"


def _is_nonneg_int(value) -> bool:
    return isinstance(value, int) and not isinstance(value, bool) and value >= 0


def _is_pos_int(value) -> bool:
    return isinstance(value, int) and not isinstance(value, bool) and value >= 1


def scan_redaction(node, path: str, errors: list) -> None:
    """specs/022 FR-002/FR-003: recursively reject identity-bearing keys,
    email/@handle/URL strings, and finer-than-month timestamps anywhere."""
    if isinstance(node, dict):
        for key, value in node.items():
            key_l = str(key).lower()
            for banned in FORBIDDEN_KEY_SUBSTRINGS:
                if banned in key_l:
                    fail(
                        errors,
                        "record.identity_key",
                        path,
                        f"Forbidden identity-bearing key '{key}' at {path} "
                        f"(contains '{banned}').",
                    )
            scan_redaction(value, f"{path}.{key}", errors)
    elif isinstance(node, list):
        for i, item in enumerate(node):
            scan_redaction(item, f"{path}[{i}]", errors)
    elif isinstance(node, str):
        if "@" in node:
            fail(
                errors,
                "record.identity_value",
                path,
                f"String at {path} contains '@' (handle/email not permitted).",
            )
        if EMAIL_RE.search(node):
            fail(
                errors,
                "record.identity_value",
                path,
                f"String at {path} looks like an email address.",
            )
        if URL_RE.search(node):
            fail(
                errors,
                "record.identity_value",
                path,
                f"String at {path} contains a URL.",
            )
        if FINE_TIMESTAMP_RE.search(node):
            fail(
                errors,
                "record.fine_timestamp",
                path,
                f"String at {path} carries a date/time finer than the "
                f"permitted YYYY-MM precision.",
            )


def validate_record(path: Path, errors: list) -> None:
    name = path.name
    m = RECORD_FILE_RE.match(name)
    if not m:
        fail(
            errors,
            "record.filename",
            path,
            f"Record files must be named pr-<n>.json; got {name}.",
        )
        return
    pr_from_name = int(m.group(1))

    try:
        data = json.loads(path.read_text())
    except (json.JSONDecodeError, OSError) as exc:
        fail(errors, "record.invalid_json", path, f"Unable to parse record: {exc}")
        return

    if not isinstance(data, dict):
        fail(errors, "record.not_object", path, "Record must be a JSON object.")
        return

    # FR-001: exact top-level field set.
    extra = set(data) - TOP_LEVEL_FIELDS
    missing = TOP_LEVEL_FIELDS - set(data)
    for field in sorted(extra):
        fail(errors, "record.unknown_field", path, f"Unknown top-level field '{field}'.")
    for field in sorted(missing):
        fail(errors, "record.missing_field", path, f"Missing required field '{field}'.")
    if extra or missing:
        # Still run the redaction scan -- it is the security-relevant check.
        scan_redaction(data, "$", errors)
        return

    if data["record_schema_version"] != RECORD_SCHEMA_VERSION:
        fail(
            errors,
            "record.schema_version",
            path,
            f"record_schema_version must be '{RECORD_SCHEMA_VERSION}'.",
        )

    # FR-001: pr_number matches the filename.
    if data["pr_number"] != pr_from_name:
        fail(
            errors,
            "record.pr_number_mismatch",
            path,
            f"pr_number {data['pr_number']!r} does not match filename pr-{pr_from_name}.json.",
        )
    if not _is_pos_int(data["pr_number"]):
        fail(errors, "record.pr_number", path, "pr_number must be a positive integer.")

    # FR-003: calendar-month period.
    if not (isinstance(data["period"], str) and PERIOD_RE.match(data["period"])):
        fail(errors, "record.period", path, "period must match YYYY-MM (calendar month).")

    # FR-004.
    if data["outcome"] not in OUTCOME_VALUES:
        fail(errors, "record.outcome", path, f"outcome must be one of {sorted(OUTCOME_VALUES)}.")
    if not _is_nonneg_int(data["hours_open_to_conclusion"]):
        fail(
            errors,
            "record.hours",
            path,
            "hours_open_to_conclusion must be a non-negative integer.",
        )
    if not _is_nonneg_int(data["review_rounds"]):
        fail(errors, "record.review_rounds", path, "review_rounds must be a non-negative integer.")

    _validate_complexity(path, data, errors)
    _validate_rubric(path, data, errors)
    _validate_ci_failure_counts(path, data, errors)

    # FR-002/FR-003: redaction scan over the whole record.
    scan_redaction(data, "$", errors)


def _validate_complexity(path: Path, data: dict, errors: list) -> None:
    complexity = data["complexity"]
    if not isinstance(complexity, dict):
        fail(errors, "record.complexity", path, "complexity must be an object.")
        return
    extra = set(complexity) - COMPLEXITY_FIELDS
    missing = COMPLEXITY_FIELDS - set(complexity)
    for field in sorted(extra):
        fail(errors, "record.complexity_unknown_field", path, f"Unknown complexity field '{field}'.")
    for field in sorted(missing):
        fail(errors, "record.complexity_missing_field", path, f"Missing complexity field '{field}'.")
    if extra or missing:
        return

    computed = complexity["computed"]
    final = complexity["final"]
    override = complexity["override"]
    reason = complexity["override_reason"]

    if computed not in COMPLEXITY_VALUES:
        fail(errors, "record.complexity_computed", path, "complexity.computed must be simple|complex.")
    if final not in COMPLEXITY_VALUES:
        fail(errors, "record.complexity_final", path, "complexity.final must be simple|complex.")
    if not isinstance(override, bool):
        fail(errors, "record.complexity_override", path, "complexity.override must be a boolean.")
        return

    # FR-005: override consistency.
    if override is False:
        if final != computed:
            fail(
                errors,
                "record.override_inconsistent",
                path,
                "override is false but final != computed.",
            )
        if reason is not None:
            fail(
                errors,
                "record.override_inconsistent",
                path,
                "override is false but override_reason is not null.",
            )
    else:
        if final == computed:
            fail(
                errors,
                "record.override_inconsistent",
                path,
                "override is true but final == computed (nothing was overridden).",
            )
        if not (isinstance(reason, str) and 1 <= len(reason) <= 200):
            fail(
                errors,
                "record.override_reason",
                path,
                "override_reason must be a non-empty string of at most 200 characters when override is true.",
            )


def _validate_rubric(path: Path, data: dict, errors: list) -> None:
    rubric = data["rubric"]
    if not isinstance(rubric, dict):
        fail(errors, "record.rubric", path, "rubric must be an object.")
        return
    extra = set(rubric) - RUBRIC_FIELDS
    missing = RUBRIC_FIELDS - set(rubric)
    for field in sorted(extra):
        fail(errors, "record.rubric_unknown_field", path, f"Unknown rubric field '{field}'.")
    for field in sorted(missing):
        fail(errors, "record.rubric_missing_field", path, f"Missing rubric field '{field}'.")
    if extra or missing:
        return

    ok = True
    for boolean_field in ("effectful", "changed_specs", "added_persona"):
        if not isinstance(rubric[boolean_field], bool):
            fail(errors, "record.rubric_type", path, f"rubric.{boolean_field} must be a boolean.")
            ok = False
    if not _is_nonneg_int(rubric["dependency_count"]):
        fail(errors, "record.rubric_type", path, "rubric.dependency_count must be a non-negative integer.")
        ok = False
    if not _is_nonneg_int(rubric["schema_field_count"]):
        fail(errors, "record.rubric_type", path, "rubric.schema_field_count must be a non-negative integer.")
        ok = False
    if not _is_pos_int(rubric["schema_field_threshold"]):
        fail(errors, "record.rubric_type", path, "rubric.schema_field_threshold must be a positive integer.")
        ok = False
    if not ok:
        return

    # FR-006: the record may not misreport its own rubric verdict.
    computed = data["complexity"].get("computed") if isinstance(data.get("complexity"), dict) else None
    if computed in COMPLEXITY_VALUES:
        expected = rubric_verdict(rubric)
        if expected != computed:
            fail(
                errors,
                "record.rubric_mismatch",
                path,
                f"rubric values imply complexity '{expected}' but complexity.computed is '{computed}'.",
            )


def _validate_ci_failure_counts(path: Path, data: dict, errors: list) -> None:
    counts = data["ci_failure_counts"]
    if not isinstance(counts, dict):
        fail(errors, "record.ci_failure_counts", path, "ci_failure_counts must be an object.")
        return
    for key, value in counts.items():
        if key not in CI_FAILURE_CATEGORIES:
            fail(
                errors,
                "record.ci_failure_unknown_category",
                path,
                f"Unknown ci_failure_counts category '{key}'. Allowed: {sorted(CI_FAILURE_CATEGORIES)}.",
            )
        if not _is_nonneg_int(value):
            fail(
                errors,
                "record.ci_failure_count_type",
                path,
                f"ci_failure_counts['{key}'] must be a non-negative integer.",
            )


def iter_record_paths(records_dir: Path = RECORDS_DIR):
    if not records_dir.is_dir():
        return []
    return sorted(
        p
        for p in records_dir.iterdir()
        if p.is_file() and p.suffix == ".json" and p.name != "schema.json"
    )


def check_record_immutability(base_sha: str, head_sha: str, errors: list) -> None:
    """specs/022 FR-011: a merged pr-<n>.json record is never modified or deleted."""
    out = subprocess.check_output(
        ["git", "diff", "--name-status", f"{base_sha}...{head_sha}", "--", str(RECORDS_DIR)],
        text=True,
    )
    for line in out.splitlines():
        line = line.strip()
        if not line:
            continue
        parts = line.split("\t")
        status = parts[0]
        target = parts[-1]
        name = Path(target).name
        if not RECORD_FILE_RE.match(name):
            continue
        if status.startswith("M") or status.startswith("D") or status.startswith("R"):
            fail(
                errors,
                "record.immutable",
                target,
                f"Measurement record {target} was {('deleted' if status.startswith('D') else 'modified')}; "
                f"records are immutable once merged -- contribute a new pr-<n>.json instead.",
            )


def main() -> int:
    errors: list = []

    for record_path in iter_record_paths(RECORDS_DIR):
        validate_record(record_path, errors)

    if len(sys.argv) >= 3:
        base_sha, head_sha = sys.argv[1], sys.argv[2]
        try:
            check_record_immutability(base_sha, head_sha, errors)
        except subprocess.CalledProcessError as exc:
            fail(errors, "git.diff_failed", str(RECORDS_DIR), f"Unable to compute diff: {exc}")

    status = "passed" if not errors else "failed"
    print(json.dumps({"status": status, "failures": errors}, indent=2))
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
