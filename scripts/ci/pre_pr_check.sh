#!/usr/bin/env bash
# Runs, locally, the same two required CI gates a capability/persona-publishing
# PR must pass: capability-validation and spec-alignment. See
# docs/decision-log.md entry 61 for why this exists -- traverse-cli's own
# `capability publish` local validation predates specs 017-persona-registry
# and the FR-020 inventory requirement, so it can report "passed" on a PR
# that CI then rejects. This script is the authoritative local reproduction:
# same scripts, same BASE_SHA/HEAD_SHA computation CI uses.
set -euo pipefail

PR_BODY_FILE="${1:-}"
if [[ -z "${PR_BODY_FILE}" ]]; then
  echo "Usage: $0 <pr-body-file>" >&2
  echo "  <pr-body-file>: path to your draft PR description, including its" >&2
  echo "  '## Governing Spec' section -- same content you'll paste into the" >&2
  echo "  GitHub PR body." >&2
  exit 1
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

git fetch origin main --quiet
BASE_SHA="$(git merge-base origin/main HEAD)"
HEAD_SHA="$(git rev-parse HEAD)"

echo "== capability-validation (BASE=${BASE_SHA:0:12} HEAD=${HEAD_SHA:0:12}) =="
python3 scripts/ci/capability_validation.py "$BASE_SHA" "$HEAD_SHA"

echo
echo "== spec-alignment =="
BASE_SHA="$BASE_SHA" HEAD_SHA="$HEAD_SHA" bash scripts/ci/spec_alignment_check.sh "$PR_BODY_FILE"

echo
echo "All local pre-PR checks passed -- CI's capability-validation and spec-alignment gates should be green."
