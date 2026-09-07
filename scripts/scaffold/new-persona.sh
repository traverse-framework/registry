#!/usr/bin/env bash
# Scaffold personas/<id>/<version>/persona.json (registry#189 / spec 017).
# Thin wrapper around scripts/scaffold/new_persona.py — pass flags through, or
# run with no args for interactive prompts.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
exec python3 "$ROOT/scripts/scaffold/new_persona.py" "$@"
