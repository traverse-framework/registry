#!/usr/bin/env python3
"""Scaffold personas/<id>/<version>/persona.json and validate it locally.

Implements the authoring helper for registry#189 / spec 017-persona-registry:
creates a well-formed persona record, requires non-empty summary/description,
requires at least one distinguished_from entry against an existing persona id
(when any personas are already registered), then runs
capability_validation.validate_persona plus distinguished_from resolution
against the on-disk personas/ tree.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
PERSONAS_DIR = REPO_ROOT / "personas"
VALIDATION_PATH = REPO_ROOT / "scripts" / "ci" / "capability_validation.py"

KEBAB_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
SEMVER_RE = re.compile(
    r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$"
)


def load_validation_module():
    spec = importlib.util.spec_from_file_location("capability_validation", VALIDATION_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"Unable to load validator from {VALIDATION_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def prompt(label: str, default: str | None = None) -> str:
    suffix = f" [{default}]" if default else ""
    value = input(f"{label}{suffix}: ").strip()
    if not value and default is not None:
        return default
    return value


def existing_persona_ids() -> list[str]:
    if not PERSONAS_DIR.is_dir():
        return []
    ids = []
    for path in sorted(PERSONAS_DIR.rglob("persona.json")):
        try:
            persona = json.loads(path.read_text())
        except Exception:
            continue
        persona_id = persona.get("id")
        if isinstance(persona_id, str) and persona_id:
            ids.append(persona_id)
    return sorted(set(ids))


def parse_distinguished_from(raw_entries: list[str]) -> list[dict]:
    entries = []
    for raw in raw_entries:
        if ":" not in raw:
            raise ValueError(
                f"distinguished_from entry must be 'persona_id:how', got {raw!r}"
            )
        persona_id, how = raw.split(":", 1)
        persona_id = persona_id.strip()
        how = how.strip()
        if not persona_id or not how:
            raise ValueError(
                f"distinguished_from entry must have non-empty persona_id and how: {raw!r}"
            )
        entries.append({"persona_id": persona_id, "how": how})
    return entries


def collect_interactive(args: argparse.Namespace) -> dict:
    persona_id = args.id or prompt("Persona id (kebab-case)")
    version = args.version or prompt("Version", "1.0.0")
    name = args.name or prompt("Display name")
    summary = args.summary or prompt("Summary (one sentence)")
    description = args.description or prompt("Description (fuller paragraph)")

    known = existing_persona_ids()
    distinguished_from = list(args.distinguished_from or [])
    if not distinguished_from:
        if known:
            print("Existing persona ids:")
            for known_id in known:
                print(f"  - {known_id}")
            print("Add at least one distinguished_from entry as persona_id:how")
            while True:
                raw = prompt("distinguished_from (empty line to finish)")
                if not raw:
                    break
                distinguished_from.append(raw)
        else:
            print("No personas registered yet; distinguished_from may be empty for the first persona.")

    return {
        "id": persona_id,
        "version": version,
        "name": name,
        "summary": summary,
        "description": description,
        "distinguished_from_raw": distinguished_from,
    }


def build_persona(payload: dict) -> dict:
    persona_id = payload["id"].strip()
    version = payload["version"].strip()
    name = payload["name"].strip()
    summary = payload["summary"].strip()
    description = payload["description"].strip()

    if not KEBAB_RE.match(persona_id):
        raise ValueError(f"persona id must be kebab-case, got {persona_id!r}")
    if not SEMVER_RE.match(version):
        raise ValueError(f"version must be exact semver X.Y.Z, got {version!r}")
    if not name:
        raise ValueError("name must be non-empty")
    if not summary:
        raise ValueError("summary must be non-empty")
    if not description:
        raise ValueError("description must be non-empty")

    distinguished_from = parse_distinguished_from(payload["distinguished_from_raw"])
    known = existing_persona_ids()
    other_ids = [persona for persona in known if persona != persona_id]
    if other_ids and not distinguished_from:
        raise ValueError(
            "at least one distinguished_from entry is required when other personas exist"
        )
    for entry in distinguished_from:
        ref = entry["persona_id"]
        if ref == persona_id:
            raise ValueError("distinguished_from cannot reference the persona being created")
        if ref not in known:
            raise ValueError(
                f"distinguished_from persona_id {ref!r} is not a registered persona"
            )

    return {
        "id": persona_id,
        "version": version,
        "name": name,
        "summary": summary,
        "description": description,
        "distinguished_from": distinguished_from,
    }


def write_persona(persona: dict) -> Path:
    path = PERSONAS_DIR / persona["id"] / persona["version"] / "persona.json"
    if path.exists():
        raise FileExistsError(f"Refusing to overwrite existing persona at {path}")
    path.parent.mkdir(parents=True, exist_ok=False)
    path.write_text(json.dumps(persona, indent=2) + "\n")
    return path


def validate_written_persona(path: Path) -> list:
    module = load_validation_module()
    errors: list = []
    module.validate_persona(path, errors)
    # Re-run tree-level distinguished_from resolution so the new file is judged
    # the same way CI will judge it after the PR lands.
    module.check_persona_distinguished_from_resolves(errors)
    # Only surface failures for the persona we just wrote (or global tree errors
    # that mention it). Pre-existing failures elsewhere should not block scaffold.
    scoped = [
        error
        for error in errors
        if error.get("path") == str(path) or path.as_posix() in str(error.get("path", ""))
    ]
    return scoped


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Scaffold a personas/<id>/<version>/persona.json and validate it."
    )
    parser.add_argument("--id", help="kebab-case persona id")
    parser.add_argument("--version", default=None, help="semver version (default: 1.0.0)")
    parser.add_argument("--name", help="human-readable display name")
    parser.add_argument("--summary", help="one-sentence summary")
    parser.add_argument("--description", help="fuller description paragraph")
    parser.add_argument(
        "--distinguished-from",
        action="append",
        default=[],
        metavar="PERSONA_ID:HOW",
        help="distinction entry; repeatable. Required when other personas exist.",
    )
    parser.add_argument(
        "--non-interactive",
        action="store_true",
        help="fail instead of prompting when required fields are missing",
    )
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    try:
        if args.non_interactive:
            missing = [
                name
                for name, value in [
                    ("--id", args.id),
                    ("--name", args.name),
                    ("--summary", args.summary),
                    ("--description", args.description),
                ]
                if not value
            ]
            if missing:
                raise ValueError(
                    "missing required flags in --non-interactive mode: " + ", ".join(missing)
                )
            if args.version is None:
                args.version = "1.0.0"
            payload = {
                "id": args.id,
                "version": args.version,
                "name": args.name,
                "summary": args.summary,
                "description": args.description,
                "distinguished_from_raw": list(args.distinguished_from or []),
            }
        else:
            payload = collect_interactive(args)
            if not payload.get("version"):
                payload["version"] = "1.0.0"

        persona = build_persona(payload)
        path = write_persona(persona)
        errors = validate_written_persona(path)
    except (ValueError, FileExistsError, RuntimeError, OSError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1

    if errors:
        print(f"Wrote {path} but local validation failed:", file=sys.stderr)
        print(json.dumps(errors, indent=2), file=sys.stderr)
        return 1

    print(f"Wrote {path}")
    print("Local validate_persona + distinguished_from resolution: passed")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
