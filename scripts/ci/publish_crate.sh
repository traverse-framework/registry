#!/usr/bin/env bash

set -euo pipefail

if [[ -z "${CARGO_REGISTRY_TOKEN:-}" ]]; then
  echo "CARGO_REGISTRY_TOKEN is not set. Grant this repo access to the org's" >&2
  echo "crates.io publish token (traverse-framework org settings) before" >&2
  echo "pushing a version tag -- see docs/decision-log.md entry 29." >&2
  exit 1
fi

repo_root="$(git rev-parse --show-toplevel)"
cd "${repo_root}"

cargo publish -p traverse-registry --locked
