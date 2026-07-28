#!/usr/bin/env bash
#
# Isolated-consumer smoke test for the published traverse-registry crate
# (Project 3 draft ticket "Verify published traverse-registry crate through
# an isolated consumer"; see #69 section 5.1). A plain `cargo test` inside
# this workspace never catches the class of bug this guards against --
# CARGO_MANIFEST_DIR resolves the same way whether or not the crate is
# actually consumable as an external dependency (see #67/#68, the federation
# governance-check regression). This creates a real, isolated temporary
# Cargo project outside the repo, depends on the crate exactly the way any
# other consumer would (a plain crates.io version requirement, no path/git
# override), and exercises real public API surface -- including the exact
# governed-path check that regressed before #67.
#
# Read-only: no publish, no version bump, no registry index mutation.

set -euo pipefail

crate_name="traverse-registry"

version="$(curl -fsS -H "User-Agent: registry-ci-verify-external-consumer" \
  "https://crates.io/api/v1/crates/${crate_name}" \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["crate"]["max_version"])')"

if [[ -z "${version}" ]]; then
  echo "Could not resolve ${crate_name}'s current published version from crates.io." >&2
  exit 1
fi

echo "Resolved ${crate_name} ${version} from crates.io (source: https://crates.io/api/v1/crates/${crate_name})."

work_dir="$(mktemp -d)"
trap 'rm -rf "${work_dir}"' EXIT

mkdir -p "${work_dir}/src"

cat > "${work_dir}/Cargo.toml" <<EOF
[package]
name = "registry-external-consumer-check"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
${crate_name} = "=${version}"
EOF

cat > "${work_dir}/src/main.rs" <<'EOF'
fn main() {
    // Basic public API construction.
    let _registry = traverse_registry::CapabilityRegistry::new();

    // The exact regression class fixed by registry#67/#68: a governed path
    // must resolve as governed from an external, non-workspace consumer.
    let cargo_toml_governed = traverse_registry::is_governed_artifact_path("Cargo.toml");
    let random_path_governed =
        traverse_registry::is_governed_artifact_path("definitely/not/a/governed/path");

    println!("cargo_toml_governed: {cargo_toml_governed}");
    println!("random_path_governed: {random_path_governed}");

    assert!(
        cargo_toml_governed,
        "Cargo.toml should be governed (spec 010) -- if this fails, the crate's \
         governing-spec/governed-path data is not resolving correctly for an \
         external consumer (the exact regression #67/#68 fixed)."
    );
    assert!(
        !random_path_governed,
        "an arbitrary ungoverned path must not be reported as governed -- \
         fail-closed behavior regressed."
    );

    println!("registry-external-consumer-check: ok");
}
EOF

echo "Building and running an isolated consumer of ${crate_name} ${version}..."
if ! (cd "${work_dir}" && cargo run 2>&1); then
  echo "FAILED: isolated external-consumer check failed for ${crate_name} ${version}." >&2
  echo "Resolution source: https://crates.io/api/v1/crates/${crate_name}" >&2
  exit 1
fi

echo "Isolated external-consumer check passed for ${crate_name} ${version}."
