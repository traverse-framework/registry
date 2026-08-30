# Feature Specification: Host-Load Trust Boundary Adoption

**Feature Branch**: `021-host-load-trust-boundary-adoption`
**Created**: 2026-08-30
**Status**: Approved (2026-08-30, v1.0.0)
**Input**: Registry-side adoption of `traverse-framework/traverse`'s `Spec 127` ("Host-Load Trust Boundary for Governed Public Bundles"), approved 2026-08-29 (`traverse#1220`), to unblock `traverse#1219` and `traverse-framework/registry#328`. The behavior change lands in `crates/traverse-registry/`'s `load_registry_bundle` / bundle-registration validation, which is registry-owned code; `traverse#1219` surfaced that `traverse` Spec 127 governs only `traverse` CLI paths and the registry's `037`/`055` entries are compatibility passthroughs, so a dedicated registry spec is required per `013-inherited-registry-governance` FR-002.

**Approval note**: the repo owner approved this adoption directly and standalone in the `registry-ops` session on 2026-08-30 (`docs/decision-log.md` entry 80), grounded in the already-owner-approved `traverse` Spec 127. Registered straight into `specs[]` with `status: approved` (no `draft_specs[]` detour, since the approval was explicit before this spec was drafted -- same path `016-ecca-event-product-adoption` v2.1.0 used).

## Why This Is An Adoption, Not A Fresh Spec

`traverse` Spec 127 is owner-authored and approved in `traverse-framework/traverse`. This spec does not re-litigate its trust-boundary decision; it translates the subset that is `traverse-registry` crate behavior -- what `load_registry_bundle` re-validates when registering a `scope: "public"` bundle -- into this repo's own spec-numbered governance, the same way `016` translated `traverse` Spec 534. The parts of Spec 127 that are `traverse` concerns (the `serve` wiring, the FR-005 end-to-end conformance test, and Spec 127 FR-004's consumer-side no-recompute rule as it applies to `traverse-cli`) stay in `traverse` under Spec 127. This repo's own publish-time gate (`scripts/ci/capability_validation.py`) is untouched.

## Purpose

Define what `crates/traverse-registry/`'s bundle-registration validation
(`load_registry_bundle` and the compatibility checks it drives) re-derives for a
`scope: "public"` bundle synced from a governed registry. An immutable
capability version published by this registry has already passed this repo's
deterministic publish gate (schema, semver-bump-vs-diff-class, digest,
dependency resolvability, Ed25519 signature, immutability); those properties are
fixed for that version's lifetime. A host loading a signed public bundle is
entitled to assume they hold and re-verify only integrity, authenticity, and
executability -- not re-run this registry's publish-time governance.

## Requirements

- **FR-001**: For a registry bundle whose `scope` is `public`, bundle-registration
  validation in `traverse-registry` MUST NOT run cross-version semver-progression
  validation (`validate_semver_progression`), contract-diff compatibility
  classification, or dependency-policy admissibility checks across the bundle's
  capability versions. It MUST retain, per capability version: artifact digest
  verification, Ed25519 artifact-signature verification (`007-artifact-hosting`
  amendment / `traverse` Spec 124), host-ABI compatibility, and contract schema
  parse.
- **FR-002**: `scope: "private"` and workspace bundles MUST retain the crate's
  current full bundle-registration validation behavior, unchanged. This spec
  changes nothing for locally authored content, which has no external publish
  gate to delegate to.
- **FR-003**: The FR-001 reduced-validation path MUST be gated on verifiable
  signed provenance: every capability in the bundle carries Ed25519 signature
  evidence (the `signature.json` sibling / artifact-state signature). A
  `public`-scope bundle in which any capability lacks that evidence MUST receive
  full validation, not the reduced path.
- **FR-004**: This spec changes only bundle registration/load validation. It
  MUST NOT change `registry materialize`'s digest/signature verification, this
  repo's publish-time `capability_validation.py` gate, or execution-time
  artifact-signature enforcement (`traverse` Spec 030).
- **FR-005**: For the `traverse-registry`-owned portion, behavior MUST match
  `traverse` Spec 127 FR-001 through FR-003. Where the two diverge, `traverse`
  Spec 127 is authoritative and this spec is amended to match.

## Success Criteria

- **SC-001**: A `scope: "public"` bundle containing a capability id with three or
  more non-deprecated versions whose consecutive contract diff classifies as
  `Unknown` registers successfully; the progression check is not run.
- **SC-002**: The same bundle still fails registration if any capability's
  artifact digest or Ed25519 signature does not verify.
- **SC-003**: A `scope: "private"` bundle with a semver-progression violation
  between two locally authored versions still fails registration exactly as
  before this spec.
- **SC-004**: A `scope: "public"` bundle in which one capability lacks signature
  evidence receives full validation (FR-003).

## Governing Relationship

Layered on the existing `crates/traverse-registry/` governance
(`011`/`012`/`013`/`014`) as a new dedicated spec per `013` FR-002. Adopts
`traverse` Spec 127; does not supersede any existing registry spec.

## Out of Scope

- The `serve` wiring and the FR-005 end-to-end conformance test (`traverse` Spec
  127 / `traverse#1219`).
- Recording per-version `change_class` / `declared_bump` in this repo's public
  index so consumers need not recompute (`traverse` Spec 127 FR-004's
  registry-side half) -- a follow-up under `003-index-release-pipeline` /
  `009-contract-metadata-in-index`.
- Workflows in the public sync path.
