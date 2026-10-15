# Changelog

## Unreleased

### Added

- **Artifact reuse marker (registry#634).** Contracts whose artifact release
  tag version differs from `contract.version` must declare
  `artifact_reuse` (or sibling `artifact_reuse.json`). CI enforces digest
  equality; catalog pages surface the marker. See decision-log entry 134.

- **Host-limits CI gate (registry#635).** Capability wasm32 fixtures execute
  under stock Traverse `WasmExecutor` defaults (32 MiB / 5_000_000 fuel) via
  `scripts/ci/wasm32_fixture_runner`. Over-limit artifacts fail as
  `capability.exceeds_host_limits`. Nightly workflow
  `catalog-host-limits` scans the live catalog; known over-limit entries are
  listed in `catalog/host-limits-allowlist.json`. See decision-log entry 132 and PR #638.
