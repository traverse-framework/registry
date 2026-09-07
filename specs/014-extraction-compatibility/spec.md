# Feature Specification: Extraction Compatibility

**Feature Branch**: `014-extraction-compatibility`
**Created**: 2026-07-21
**Status**: Approved
**Input**: Prerequisite work for #9 (extract the `traverse-registry` crate from `traverse-framework/traverse` into this repo). Decided via `/brainstorm` with the repo owner, 2026-07-21. Full reasoning: `docs/decision-log.md` entry 33.

## Purpose

The extracted crate resolves its own governing-spec registry at **compile time**, relative to its own directory:

```rust
const APPROVED_SPECS_REGISTRY_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../specs/governance/approved-specs.json");
```

Two runtime mechanisms read that file:

- `approved_spec_registry_contains(spec_id)` — checks a `governing_spec` string embedded in federation/export records against the `id` field of every entry in `specs[]`.
- `is_governed_artifact_path(path)` — checks a path string against every entry's `governs` array of path prefixes.

Both currently resolve to `traverse`'s own `specs/governance/approved-specs.json`. Once `crates/traverse-registry/` physically relocates to this repo, the identical relative path resolves to **this repo's** governance file instead — a different file with different entries. Without this spec, both mechanisms would silently start failing for pre-existing, unchanged behavior, purely because of the physical move, not because of any actual behavior change.

This spec exists to make that relocation behavior-neutral. It does not change the crate's code, and it does not introduce a migration/retirement mechanism for the legacy IDs it covers — per `013-inherited-registry-governance`'s own precedent, inherited behavior is preserved as-is, not re-authored.

## Investigation (2026-07-21 audit)

A full audit of the crate's compiled source (all files under `crates/traverse-registry/src/`) found every literal spec-ID-shaped string the crate embeds:

`002-capability-contracts`, `005-capability-registry`, `007-workflow-registry-traversal`, `011-event-registry`, `015-metadata-graph`, `026-federation-registry-routing`, `037-semver-range-resolution`, `039-connector-plugin-architecture`, `043-module-dependency-management`, `044-application-bundle-manifest`, `046-public-cli-app-registration`, `055-registry-sync`, `999-unapproved-spec`.

Cross-checked against `traverse`'s own current `specs/governance/approved-specs.json`:

- **10 are real and currently approved** in `traverse` today: `002`, `005`, `007`, `011`, `037`, `039`, `043`, `044`, `046`, `055`. These need passthrough entries here (FR-001).
- **`999-unapproved-spec` is a deliberate negative-test fixture** (used in `traverse`'s own test suite to confirm rejection of unknown IDs works). It must never be added as an approved entry, here or anywhere (FR-003).
- **`015-metadata-graph` and `026-federation-registry-routing` do not exist in `traverse`'s current governance file at all.** `015` is actually `015-capability-discovery-mcp` in `traverse` today; `026` is `026-event-broker`. These two constants are stale, pre-existing references in `traverse`'s own source — `approved_spec_registry_contains` already returns `false` for them **today, before any extraction**. This is a latent `traverse`-side data-quality issue, unrelated to this extraction. It is explicitly out of scope here: adding entries for IDs that were never valid would not preserve prior behavior, it would silently change it (from "correctly rejected" to "incorrectly accepted"). Flagged separately for `traverse` to address on its own side.

Additionally, this audit found `002-capability-contracts` governs the crate but was missing from `013-inherited-registry-governance`'s source-specs table (which lists 19 specs, audited 2026-07-18) — a gap in that table's original audit, corrected by this spec (see "Amendment to 013" below).

## Requirements

### Functional Requirements

- **FR-001**: This repo's `specs/governance/approved-specs.json` MUST include one minimal passthrough entry per verified-real legacy ID above (`002-capability-contracts`, `005-capability-registry`, `007-workflow-registry-traversal`, `011-event-registry`, `037-semver-range-resolution`, `039-connector-plugin-architecture`, `043-module-dependency-management`, `044-application-bundle-manifest`, `046-public-cli-app-registration`, `055-registry-sync`), each with `status: approved`, `immutable: true`, and `governs: ["crates/traverse-registry/"]`, so `approved_spec_registry_contains` continues returning `true` for these exact literal strings without any Rust source change.
- **FR-002**: This spec's own `approved-specs.json` entry MUST include `contracts/` in its `governs` list (in addition to `crates/traverse-registry/`), preserving `is_governed_artifact_path`'s existing classification of that path prefix as governed.
- **FR-003**: `999-unapproved-spec` MUST NOT be added as an approved entry, here or via any future amendment to this spec.
- **FR-004**: The legacy-ID list in FR-001 MUST be re-verified against the crate's actual compiled source immediately before this spec's real crate-content lands (mirroring `013-inherited-registry-governance` FR-003's re-verification requirement), since this snapshot is dated 2026-07-21 and the crate's source may change before physical extraction.
- **FR-005**: This spec MUST NOT be used to justify any behavior change to the extracted crate; it exists solely to preserve pre-existing behavior across the physical relocation (mirrors `013-inherited-registry-governance` FR-002).

### Amendment to `013-inherited-registry-governance`

Licensed by that spec's own FR-003 ("MUST be re-verified... and updated if new specs have started governing `crates/traverse-registry/`"): `002-capability-contracts` is added to its source-specs table, correcting a gap in the original 2026-07-18 audit (19 specs found; `002` was a 20th, missed at the time).

## Success Criteria

- **SC-001**: `approved_spec_registry_contains` and `is_governed_artifact_path` return identical results for every currently-exercised call site, before and after the crate's physical relocation.
- **SC-002**: No new spec content is authored for the 10 legacy IDs beyond the minimal passthrough entry — this spec does not re-litigate or restate their original requirements (consistent with `013`'s blanket-coverage precedent).
- **SC-003**: `015-metadata-graph` and `026-federation-registry-routing` remain unregistered here; their pre-existing `traverse`-side breakage is neither hidden nor "fixed" by this spec.

## Governing Relationship

This specification governs `crates/traverse-registry/` and the `contracts/` path-prefix concept in this repo, specifically for the extraction-compatibility passthrough entries described above. It is deliberately narrow and additive to `013-inherited-registry-governance`, not a replacement for it.
