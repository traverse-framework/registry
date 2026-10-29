# Changelog

## Unreleased

### Fixed

- **Embed approved-specs registry at compile time (registry#628).**
  `is_governed_artifact_path` / approved-spec checks no longer `fs::read`
  an absolute `CARGO_MANIFEST_DIR` path at runtime (which failed open for
  governed-path classification on every shipped host). The registry is
  now `include_str!`-embedded; classification is deterministic and
  portable to iOS/Android/wasm32/installed CLI.

### Added

- **`model.execute@1.1.0` (registry#641).** Normative flat `model_evidence`
  output schema (`license_id`, `commercial_use`, `status`, `effective_usage`;
  optional `derivation` without URLs). Reuses the `1.0.0` WASM artifact.
  Aligns catalog demos with Spec 104 (no host-private URLs in guest evidence).

- **`model.execute@1.0.0` (registry#633).** Catalog capability over
  `traverse.model-runtime` / Spec 138 exact-ref model execution. See
  decision-log entry 133 and draft spec `027-catalog-model-execute`.

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
