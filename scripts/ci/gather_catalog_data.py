#!/usr/bin/env python3
"""Gather published capability metadata for the discovery catalog.

Implements the "gather script" half of registry#105 (child of #103, the
capability-discovery umbrella; design decided via `/brainstorm`,
2026-07-29, decision-log entry 40). The `catalog-builder` Traverse
capability (`capability-src/catalog-builder/`) cannot walk `capabilities/`
itself -- the governed WASM ABI only allows a single input/single output
via `fd_read`/`fd_write`, no directory listing, no filesystem access -- so
this plain script does the tree walk and hands the capability one JSON
object to transform: `{"capabilities": [...], "personas": [...], "events": [...]}`
(the `personas` array added for specs/017-persona-registry, decision-log entry
53 -- same "carry the entire record" reasoning as capabilities, below; the
`events` array added for foundation FR-016 / registry#168 so catalog-builder
can later render ECCA event products).

Mirrors scripts/ci/build_index.py's walk (every capabilities/**/contract.json,
including deprecated versions, each carrying its own `deprecated` flag) --
deliberately not filtered here, so the catalog-builder capability (or a
future template) decides what to do with deprecated entries, rather than
this script silently deciding for it.

Carries the *entire* contract.json per entry, not a hand-picked field
subset -- the catalog's per-capability detail page needs "all the infos"
(inputs/outputs schema, use_cases, owner, provenance, artifact, etc.), and
re-curating a field list here every time the contract schema grows is
exactly the kind of drift this script should not own.

Also attaches real, measured `test_coverage` (line/function/region percent
via `cargo llvm-cov`, plus a test count) to whichever version of each
capability id is the *current* one -- i.e. the version whose logic still
lives in `capability-src/`. Deliberately not fabricated and not applied to
older/deprecated versions: their real implementation isn't retained
separately in this repo (only the current logic is), so there is nothing
honest to measure for them.

Usage: gather_catalog_data.py <output_path>
"""

import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Optional

# Maps a capability id to the capability-src/ crate whose current source
# backs it. Only the 11 real, tested capabilities published today have an
# entry -- a future capability must be added here explicitly, the same
# deliberate-no-magic policy verify_use_cases.py (registry#107) used for its
# own binary-path mapping.
CURRENT_CRATE_FOR_ID = {
    "validation.validate-luhn": "validate-luhn",
    "validation.validate-email": "validate-email",
    "validation.normalize-phone-number": "normalize-phone-number",
    "validation.score-password-strength": "score-password-strength",
    "formatting.format-currency": "format-currency",
    "traverse-starter.process": "traverse-starter-process",
    "traverse-starter.validate": "traverse-starter-validate",
    "traverse-starter.summarize": "traverse-starter-summarize",
    "doc-approval.analyze": "doc-approval-analyze",
    "doc-approval.recommend": "doc-approval-recommend",
    "meeting-notes.process": "meeting-notes-process",
}


def semver_tuple(version: str) -> tuple:
    return tuple(int(part) for part in version.split("."))


def count_test_functions(crate_dir: str) -> int:
    main_rs = Path("capability-src") / crate_dir / "src" / "main.rs"
    if not main_rs.is_file():
        return 0
    return len(re.findall(r"#\[test\]", main_rs.read_text()))


def measure_test_coverage(crate_dir: str) -> Optional[dict]:
    manifest_path = Path("capability-src") / crate_dir / "Cargo.toml"
    if not manifest_path.is_file():
        return None
    try:
        result = subprocess.run(
            [
                "cargo",
                "llvm-cov",
                "--quiet",
                "--json",
                "--summary-only",
                "--manifest-path",
                str(manifest_path),
            ],
            capture_output=True,
            text=True,
            check=True,
            timeout=120,
        )
        totals = json.loads(result.stdout)["data"][0]["totals"]
    except Exception as exc:
        print(f"warning: coverage measurement failed for {crate_dir}: {exc}", file=sys.stderr)
        return None

    return {
        "lines_percent": round(totals["lines"]["percent"], 1),
        "functions_percent": round(totals["functions"]["percent"], 1),
        "regions_percent": round(totals["regions"]["percent"], 1),
        "test_count": count_test_functions(crate_dir),
    }


def gather_capabilities() -> list:
    capabilities_dir = Path("capabilities")
    entries = []

    if not capabilities_dir.is_dir():
        return entries

    for contract_path in sorted(capabilities_dir.rglob("contract.json")):
        contract = json.loads(contract_path.read_text())
        deprecated = (contract_path.parent / "deprecated.json").is_file()

        entries.append(
            {
                "deprecated": deprecated,
                "contract": contract,
                "test_coverage": None,
            }
        )

    # Attach measured coverage only to each id's current (highest-version)
    # entry -- the one whose logic actually lives in capability-src/ today.
    latest_by_id: dict[str, dict] = {}
    for entry in entries:
        capability_id = entry["contract"]["id"]
        version = entry["contract"]["version"]
        current_latest = latest_by_id.get(capability_id)
        if current_latest is None or semver_tuple(version) > semver_tuple(current_latest["contract"]["version"]):
            latest_by_id[capability_id] = entry

    for capability_id, crate_dir in CURRENT_CRATE_FOR_ID.items():
        latest_entry = latest_by_id.get(capability_id)
        if latest_entry is not None:
            latest_entry["test_coverage"] = measure_test_coverage(crate_dir)

    return entries


def gather_personas() -> list:
    """Walks personas/**/persona.json (specs/017-persona-registry) the same
    way gather_capabilities walks capabilities/ -- carries the entire
    persona record, not a hand-picked field subset, for the same reason
    documented at this module's top for capabilities."""
    personas_dir = Path("personas")
    entries = []

    if not personas_dir.is_dir():
        return entries

    for persona_path in sorted(personas_dir.rglob("persona.json")):
        persona = json.loads(persona_path.read_text())
        entries.append({"persona": persona})

    return entries


def gather_events() -> list:
    """FR-016 / registry#160: walk events/**/product.json for catalog-builder.

    Carries the *entire* EventProductDescriptor per entry (plus a sibling
    `deprecated` flag), same "don't hand-curate a field subset" reasoning as
    gather_capabilities -- the catalog's event detail page and FR-014 filters
    (event/capability/domain/owner/lifecycle/classification) need owner,
    domain, field_classifications, support_route, etc., not just a summary.
    """
    events_dir = Path("events")
    entries = []

    if not events_dir.is_dir():
        return entries

    for product_path in sorted(events_dir.rglob("product.json")):
        product = json.loads(product_path.read_text())
        deprecated = (product_path.parent / "deprecated.json").is_file()
        entries.append(
            {
                "deprecated": deprecated,
                "product": product,
            }
        )

    return entries


def gather_catalog_data() -> dict:
    return {
        "capabilities": gather_capabilities(),
        "personas": gather_personas(),
        "events": gather_events(),
    }


def main() -> int:
    if len(sys.argv) != 2:
        print("Usage: gather_catalog_data.py <output_path>", file=sys.stderr)
        return 1

    output_path = Path(sys.argv[1])
    data = gather_catalog_data()
    output_path.write_text(json.dumps(data, indent=2) + "\n")
    print(
        f"Gathered {len(data['capabilities'])} capability record(s), "
        f"{len(data['personas'])} persona record(s), and "
        f"{len(data['events'])} event record(s) at {output_path}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
