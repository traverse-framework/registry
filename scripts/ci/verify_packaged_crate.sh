#!/usr/bin/env bash
#
# Regression guard for the federation governing-spec compatibility bug
# (registry#9 / Project 1 "Restore published registry federation spec
# compatibility"). An in-repo `cargo test -p traverse-registry` is not
# sufficient evidence here: the lib is still compiled from
# crates/traverse-registry/ inside this workspace, so `CARGO_MANIFEST_DIR`
# still resolves the same way it always has. The actual bug only surfaces
# once the crate is packaged and built from an unrelated directory, the way
# an external consumer (e.g. Traverse depending on the crates.io release)
# would build it. This script reproduces that: package the crate, extract it
# outside the repo, and run its federation test suite from there.

set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "${repo_root}"

cargo package -p traverse-registry --locked --allow-dirty

version="$(cargo metadata --no-deps --format-version 1 \
  | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "traverse-registry"))')"

crate_file="${repo_root}/target/package/traverse-registry-${version}.crate"
if [[ ! -f "${crate_file}" ]]; then
  echo "Expected packaged crate not found: ${crate_file}" >&2
  exit 1
fi

work_dir="$(mktemp -d)"
trap 'rm -rf "${work_dir}"' EXIT

tar xzf "${crate_file}" -C "${work_dir}"
extracted_dir="${work_dir}/traverse-registry-${version}"

echo "== Verifying valid governing specs sync successfully from packaged crate =="
(cd "${extracted_dir}" && cargo test --locked --lib \
  federation::tests::syncs_peer_export_and_routes_invocation_to_owner -- --exact)

echo "== Verifying an intentionally unapproved governing spec is still rejected =="
(cd "${extracted_dir}" && cargo test --locked --lib \
  federation::tests::sync_rejects_unapproved_governing_specs_with_audit_evidence -- --exact)

echo "== Verifying the full federation module (governed-path + sync/routing) still passes =="
(cd "${extracted_dir}" && cargo test --locked --lib federation::)

echo "Packaged-crate federation compatibility check passed."
