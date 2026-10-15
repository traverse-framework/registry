# Changelog

## Unreleased

### Added

- **Host-limits CI gate (registry#635).** Capability wasm32 fixtures execute
  under stock Traverse `WasmExecutor` defaults (32 MiB / 5_000_000 fuel) via
  `scripts/ci/wasm32_fixture_runner`. Over-limit artifacts fail as
  `capability.exceeds_host_limits`. Nightly workflow
  `catalog-host-limits` scans the live catalog; known over-limit entries are
  listed in `catalog/host-limits-allowlist.json`. See decision-log entry 132 and PR #638.
