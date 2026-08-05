# Feature Specification: ECCA Event-Product Adoption

**Feature Branch**: `016-ecca-event-product-adoption`
**Created**: 2026-08-05
**Status**: Approved (2026-08-05)
**Input**: Registry-side adoption of `traverse-framework/traverse`'s `Spec 534` ("ECCA Event Products") and `ADR-0028` ("ECCA Event-Product Standard"), both Accepted 2026-07-29, to unblock `traverse#896` ("Implement ECCA registry catalog and capability discovery"), a child of `traverse#894` and gated on `traverse#895` (the spec/ADR ticket, now Done). The most recent comment on `traverse#896` (from the repo owner) confirms the work belongs here: Traverse's own workspace has no registry crate. Unlike `011-capability-registry-adoption` and `013-inherited-registry-governance`, this spec was not decided via an interactive `/brainstorm` with the repo owner before drafting its functional requirements -- it was adopted directly from the ops request that triggered this work, grounded in the already-owner-approved `Spec 534`/`ADR-0028`. See the **Enforcement note** below for the approval history.

**Enforcement note**: `crates/traverse-registry/` already exists and is governed by `011-capability-registry-adoption`, `012-event-registry-adoption`, and `013-inherited-registry-governance`. This spec is a new, dedicated spec for new behavior, per `013`'s own `FR-002` ("any new requirement or behavior change from this point forward MUST go through a new, dedicated spec in this repo... not an edit to this spec or the specs it inherits from"). Registered 2026-08-05 in `specs/governance/approved-specs.json`'s `draft_specs[]` array, not `specs[]`, per the registry-ops operating model's guardrail that a spec cannot self-approve from Draft to Approved -- implementation proceeded against it while Draft (precedent: decision log entry 22, `007-artifact-hosting` implemented while Draft), registering ahead of approval being normal spec-driven order here, not a claim that it was already binding. The repo owner reviewed and approved it directly, as its own standalone decision, on 2026-08-05 (`docs/decision-log.md` entry 49) -- moved to `specs[]` with `status: approved` accordingly.

## Why This Is An Adoption, Not A Fresh Spec

`Spec 534`/`ADR-0028` are already owner-authored and Accepted in `traverse-framework/traverse`. This spec does not re-litigate their decisions; it translates the subset that is registry behavior (governed event-product descriptors, publish-time validation, declared-vs-observed lineage, catalog/discovery) into this repo's own spec-numbered governance, the same way `011`/`012` translated `traverse`'s `005`/`011-event-registry`. It does not cover the parts of `ADR-0028` that are runtime/transport/wire-protocol concerns (CloudEvents delivery, OpenTelemetry evidence collection, quarantine enforcement) -- those stay in `traverse`, out of this spec's scope, per the existing registry/runtime boundary (`013`'s own scope note).

## Purpose

This spec defines the ECCA event-product slice for `traverse-registry`, layered on top of the existing `012-event-registry-adoption` event registry rather than replacing it. `traverse-contracts` (the crate that defines `EventContract`) is an exact-pinned external dependency (`= "0.8.1"`, published from `traverse-framework/traverse`); this repo cannot add fields to `EventContract` itself. Every requirement below is additive: a registry-owned `EventProductDescriptor` that composes around an already-validated `EventContract`.

This spec covers:

- the canonical ECCA event-product descriptor: an `EventProductDescriptor` wrapping a `012`-validated `EventContract` plus ECCA-additive fields (stable support route, controlled field-level classification, lifecycle-deprecation replacement pointer)
- publish-time validation that rejects a descriptor when the support route, field classifications, lifecycle/replacement pairing, or semantic (past-tense) naming rules are absent or invalid
- immutable republication semantics for a descriptor at a given `(scope, id, version)`, consistent with `012`'s immutability model for the underlying contract
- persisted, indexed storage of declared producer/consumer relationships (already partially satisfied by `012`'s `EventRegistry.publishers`/`subscribers` -- this spec extends indexing/discovery, not the underlying storage)
- a registry-owned, structurally separate record of **observed** runtime lineage and drift evidence, isolated from declared/governed state (mirroring `015-runtime-usage-telemetry-resolve-hook`'s external, side-effect-only, "MUST NOT affect the declared path" hook shape)
- a catalog/discovery query surface exposing event contract/version/schema/purpose, owner/support route, lifecycle, exposure/field classification, compatibility, publishers/consumers, and declared-vs-observed lineage, filterable by event, capability, domain, owner, lifecycle, and classification
- AsyncAPI as a derived export of the above, never a source of truth

This spec does **not** cover:

- CloudEvents wire-format delivery, transport, or broker behavior (stays in `traverse`)
- OpenTelemetry evidence collection or runtime quarantine enforcement (stays in `traverse`; this repo only stores the resulting evidence records once produced)
- changes to `traverse-contracts`' `EventContract` type itself (out of this repo's control; exact-pinned external dependency)
- the Traverse runtime/reference-app conformance journey (`traverse#897`/`traverse#898`)

## Scope

In scope: `crates/traverse-registry/` (new modules), and this spec's own directory.

Out of scope: `traverse-contracts`, `traverse-runtime`, `traverse-cli` (all stay in `traverse`, per the existing registry/runtime boundary already established by `013`).

## Requirements

### Functional Requirements

- **FR-001**: The registry MUST define an `EventProductDescriptor` that composes an already-`012`-validated `EventContract` with ECCA-additive metadata, without modifying `EventContract` itself.
- **FR-002**: Descriptor validation MUST reject a descriptor with a missing or non-`https://` support route.
- **FR-003**: Descriptor validation MUST reject a descriptor whose field classifications do not exactly cover the top-level properties declared in the underlying contract's `payload.schema` (each declared property classified exactly once; no classification for an undeclared property).
- **FR-004**: Descriptor validation MUST require a replacement pointer when the underlying contract's lifecycle is `deprecated` or `retired`, and MUST reject a replacement pointer when lifecycle is `draft` or `active`.
- **FR-005**: Descriptor validation MUST reject a replacement pointer with an empty `event_id`/`version`, or one that is self-referential (points at the same `(id, version)` being validated).
- **FR-006**: Descriptor validation MUST reject a contract `name` whose final hyphen-segment is not past tense, using a deterministic rule (regular `-ed` suffix or a curated closed allow-list of irregular past participles) rather than a general-purpose linguistic classifier.
- **FR-007**: A previously published descriptor for the same `(scope, id, version)` MUST be immutable; validating a candidate descriptor against an existing one with different content MUST fail, and against identical content MUST succeed idempotently.
- **FR-008**: Declared producer/consumer relationships MUST remain sourced from the underlying `EventContract`'s `publishers`/`subscribers` (already governed by `012`) -- this spec MUST NOT introduce a second, divergent declared-relationship model.
- **FR-009**: Observed runtime lineage and drift evidence MUST be stored in a structure disjoint from declared/governed descriptor state, MUST be supplied only by an external, side-effect-only caller (mirroring `015`'s hook isolation), and MUST NOT be inferable from, or influence, descriptor validation.
- **FR-010**: The registry MUST expose a catalog/discovery query surface over event contract/version/schema/purpose, owner/support route, lifecycle, exposure/field classification, compatibility, publishers/consumers, and declared-vs-observed lineage, filterable by event, capability, domain, owner, lifecycle, and classification.
- **FR-011**: Any AsyncAPI representation the registry produces MUST be generated from the governed descriptor and MUST NOT be hand-authored or treated as contract authority.
- **FR-012**: Descriptor validation and catalog discovery results MUST be deterministic for identical stored records and query inputs, consistent with `012`'s `NFR-001`.

### Non-Functional Requirements

- **NFR-001 Determinism**: Descriptor validation, immutability checks, and catalog discovery MUST be deterministic for identical inputs.
- **NFR-002 Explainability**: Validation failures MUST produce stable, machine-readable evidence with an error code, path, and message, consistent with `012`'s `EventRegistryError` shape.
- **NFR-003 Isolation**: Observed-lineage/drift-evidence recording MUST be structurally isolated from declared-state validation and storage; a failure or absence of observed data MUST NOT change declared-state validation outcomes.
- **NFR-004 Testability**: Core validation and catalog-discovery logic MUST be structured for full automated coverage, independent of any WASM/host-runtime dependency.
- **NFR-005 Non-duplication**: This spec MUST reuse `012`'s existing `EventContract`/`EventRegistry` validation and storage wherever it already satisfies an ECCA requirement, rather than re-implementing it.

## Success Criteria

- **SC-001**: A valid event-product descriptor can be validated and accepted; an invalid one is rejected with a specific, addressable error for each violated rule (support route, field classification, lifecycle/replacement, naming).
- **SC-002**: Republishing an unchanged descriptor for the same `(scope, id, version)` is idempotent; republishing changed content for the same `(scope, id, version)` is rejected.
- **SC-003**: Declared and observed relationship data remain structurally distinguishable at all times -- no code path can produce declared-state output from observed-state input or vice versa.
- **SC-004**: `traverse#896`'s catalog/discovery DoD items are each traceable to one or more FRs above.

## Governing Relationship

This specification is adopted from `traverse-framework/traverse`'s `Spec 534`/`ADR-0028`, and governs `crates/traverse-registry/` in this repo for ECCA event-product behavior, layered additively on `011-capability-registry-adoption`, `012-event-registry-adoption`, and `013-inherited-registry-governance`.
