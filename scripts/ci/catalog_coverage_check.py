#!/usr/bin/env python3
"""Deterministic guard: the discovery catalog's coverage badges cannot
silently go missing again (decision-log entry 95).

Root cause it prevents
---------------------
`gather_catalog_data.py` attaches a measured `test_coverage` (the "N% covered"
badge on registry.traverse-framework.com) to a capability only if it can
resolve that capability's id to the `capability-src/` crate whose current
source backs it. For a long time the *only* resolution path was the hand-
maintained `CURRENT_CRATE_FOR_ID` dict. `specs/018-capability-test-coverage`
then mandated a canonical crate name for every new capability (id with "."
replaced by "-") but never taught `gather_catalog_data.py` about it -- so 14
capabilities published after that spec, each with a real, fully-tested crate
sitting in `capability-src/`, rendered on the public catalog with no badge
at all. Nothing pointed at their crates.

`gather_catalog_data.py` now falls back to the canonical name, so a spec-018
compliant crate is picked up automatically. This check closes the remaining
gap:

  1. A `capability-src/` capability crate that NO current capability id
     resolves to (via a dict entry or the canonical rule) is drift -- a dead
     crate, a crate misnamed relative to its id, or a capability missing its
     `CURRENT_CRATE_FOR_ID` entry -- and fails CI loudly instead of quietly
     costing a badge.

  2. A current (latest, non-deprecated) capability that resolves to NO crate
     and is not in the explicit `KNOWN_SOURCELESS` exception set fails too --
     so "this capability has no source in the repo" is always a recorded,
     reviewed decision, never an accident.

No cargo, no network: walks `capabilities/` and `capability-src/` only.
Prints {"status": ..., "failures": [...]} and exits non-zero on any failure,
the same contract as `capability_validation.py`.

Usage:
  catalog_coverage_check.py                 # whole-tree check (what CI runs)
  catalog_coverage_check.py --list          # also print the full id -> crate map
"""

import importlib.util
import json
import sys
from pathlib import Path

_MODULE_DIR = Path(__file__).resolve().parent
_GATHER_PATH = _MODULE_DIR / "gather_catalog_data.py"

# Non-capability crates under capability-src/: the shared no_std runtime shim
# and the catalog-builder capability (which transforms gathered data and has
# no capabilities/ contract of its own).
NON_CAPABILITY_CRATES = {"wasi-capability-runtime", "catalog-builder"}

# Capability ids this repo deliberately publishes with no capability-src/
# source of their own (registry#302: owned by an external team, source
# requested but not yet contributed). Each entry is an accepted, reviewed
# exception -- a *new* sourceless capability must be added here in the same
# PR, which is the review checkpoint. Empty today: every published
# capability currently has resolvable source.
KNOWN_SOURCELESS: set = set()


def _load_gather_module():
    spec = importlib.util.spec_from_file_location("gather_catalog_data", _GATHER_PATH)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def latest_contract_versions(capabilities_dir: Path, semver_tuple) -> dict:
    """{capability_id: {"version": str, "deprecated": bool}} for the highest
    version of each id -- mirrors gather_catalog_data.py's own latest-by-id
    selection."""
    latest: dict = {}
    if not capabilities_dir.is_dir():
        return latest
    for contract_path in sorted(capabilities_dir.rglob("contract.json")):
        try:
            contract = json.loads(contract_path.read_text())
        except (OSError, json.JSONDecodeError):
            continue
        capability_id = contract.get("id")
        version = contract.get("version")
        if not isinstance(capability_id, str) or not isinstance(version, str):
            continue
        deprecated = (contract_path.parent / "deprecated.json").is_file()
        current = latest.get(capability_id)
        if current is None or semver_tuple(version) > semver_tuple(current["version"]):
            latest[capability_id] = {"version": version, "deprecated": deprecated}
    return latest


def find_failures(gather, capabilities_dir=Path("capabilities"),
                  capability_src_dir=Path("capability-src")) -> list:
    failures: list = []
    latest = latest_contract_versions(capabilities_dir, gather.semver_tuple)

    # id -> resolved crate (or None) for every published id.
    resolved: dict = {cid: gather.resolve_current_crate(cid) for cid in latest}
    referenced_crates = {crate for crate in resolved.values() if crate is not None}

    # Check 1: every capability crate on disk is pointed at by some id.
    if capability_src_dir.is_dir():
        for crate_path in sorted(p for p in capability_src_dir.iterdir() if p.is_dir()):
            if not (crate_path / "Cargo.toml").is_file():
                continue
            name = crate_path.name
            if name in NON_CAPABILITY_CRATES:
                continue
            if name not in referenced_crates:
                failures.append({
                    "code": "catalog.unreferenced_capability_src_crate",
                    "path": str(crate_path),
                    "message": (
                        f"'{crate_path}' has source but no current capability resolves "
                        f"to it, so the catalog will show no coverage badge for it. Fix "
                        f"one of: (a) rename the crate to the spec-018 canonical name "
                        f"'<id-with-dots-as-dashes>'; (b) add a CURRENT_CRATE_FOR_ID "
                        f"entry in scripts/ci/gather_catalog_data.py mapping the owning "
                        f"capability id to '{name}'; (c) delete the crate if it is dead."
                    ),
                })

    # Check 2: every current, non-deprecated capability has resolvable source
    # (or a recorded exception).
    for capability_id, meta in sorted(latest.items()):
        if meta["deprecated"]:
            continue
        if resolved[capability_id] is not None:
            continue
        if capability_id in KNOWN_SOURCELESS:
            continue
        failures.append({
            "code": "catalog.current_capability_without_source",
            "path": f"capabilities/**/{capability_id}/{meta['version']}/contract.json",
            "message": (
                f"current capability '{capability_id}' resolves to no capability-src/ "
                f"crate (expected 'capability-src/{gather.canonical_crate_for_id(capability_id)}/"
                f"Cargo.toml' or a CURRENT_CRATE_FOR_ID entry), so it renders with no "
                f"coverage badge. Add the crate, or add '{capability_id}' to "
                f"KNOWN_SOURCELESS in scripts/ci/catalog_coverage_check.py if it is a "
                f"reviewed registry#302-style exception."
            ),
        })

    return failures


def main() -> int:
    gather = _load_gather_module()

    if "--list" in sys.argv[1:]:
        latest = latest_contract_versions(Path("capabilities"), gather.semver_tuple)
        for capability_id in sorted(latest):
            crate = gather.resolve_current_crate(capability_id)
            marker = "" if latest[capability_id]["deprecated"] else " *"
            print(f"{capability_id:48s} -> {crate or '(no source)'}{marker}")
        print("\n* = current, non-deprecated (badge expected)", file=sys.stderr)

    failures = find_failures(gather)
    status = "passed" if not failures else "failed"
    print(json.dumps({"status": status, "failures": failures}, indent=2))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
