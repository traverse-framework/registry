#!/usr/bin/env python3
"""Gather published capability metadata for the discovery catalog.

Implements the "gather script" half of registry#105 (child of #103, the
capability-discovery umbrella; design decided via `/brainstorm`,
2026-07-29, decision-log entry 40). The `catalog-builder` Traverse
capability (`capability-src/catalog-builder/`) cannot walk `capabilities/`
itself -- the governed WASM ABI only allows a single input/single output
via `fd_read`/`fd_write`, no directory listing, no filesystem access -- so
this plain script does the tree walk and hands the capability one flat
JSON array to transform.

Mirrors scripts/ci/build_index.py's walk (every capabilities/**/contract.json,
including deprecated versions, each carrying its own `deprecated` flag) --
deliberately not filtered here, so the catalog-builder capability (or a
future template) decides what to do with deprecated entries, rather than
this script silently deciding for it.

Usage: gather_catalog_data.py <output_path>
"""

import json
import sys
from pathlib import Path


def gather_catalog_data() -> list:
    capabilities_dir = Path("capabilities")
    entries = []

    if not capabilities_dir.is_dir():
        return entries

    for contract_path in sorted(capabilities_dir.rglob("contract.json")):
        contract = json.loads(contract_path.read_text())
        deprecated = (contract_path.parent / "deprecated.json").is_file()

        entries.append(
            {
                "namespace": contract.get("namespace"),
                "id": contract.get("id"),
                "version": contract.get("version"),
                "summary": contract.get("summary"),
                "description": contract.get("description"),
                "use_cases": contract.get("use_cases"),
                "deprecated": deprecated,
            }
        )

    return entries


def main() -> int:
    if len(sys.argv) != 2:
        print("Usage: gather_catalog_data.py <output_path>", file=sys.stderr)
        return 1

    output_path = Path(sys.argv[1])
    entries = gather_catalog_data()
    output_path.write_text(json.dumps(entries, indent=2) + "\n")
    print(f"Gathered {len(entries)} capability record(s) at {output_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
