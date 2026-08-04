# Feature Specification: Runtime Usage Telemetry Resolve Hook

**Feature Branch**: `015-runtime-usage-telemetry-resolve-hook`
**Created**: 2026-08-04
**Status**: Approved
**Input**: Decided via `/brainstorm` with the repo owner, closing #134. Full reasoning: `docs/decision-log.md` entry 47. Handed off from that entry as a ready spec proposal to `traverse-framework/traverse`, which codified the cross-repo architecture as its own Spec 088 (`specs/536-runtime-usage-telemetry`, ADR-0030, that repo's decision-log entry 42).

## Purpose

`crates/traverse-registry/` was extracted from `traverse-framework/traverse` (Spec 051) and is governed here under `013-inherited-registry-governance`, whose FR-002 requires any *new* behavior change to the extracted crate to go through a dedicated spec in this repo, not an edit to that inherited-governance list. This spec is that dedicated spec: it authorizes and defines the one real behavior change decision 47 / traverse Spec 088 requires of this crate — calling an externally-defined, optional telemetry port on a successful capability resolution.

This spec does not define the port trait itself, the collector, the consent model, or any network/adapter code — those are `traverse-contracts`/`traverse-cli` concerns, governed by and implemented in `traverse-framework/traverse` (that repo's Spec 088). This spec defines only the contract this crate's resolution path must satisfy, and constrains it so the crate's portability and offline testability are preserved.

## Scope

In scope:

- the requirement that `crates/traverse-registry`'s resolution path accept an optional, caller-supplied telemetry sink and invoke it on successful resolution
- the constraint that this crate never depends on a concrete network client, opt-in state, or collector configuration

Out of scope:

- the `UsageTelemetrySink` trait's definition (lives in `traverse-contracts`, an external pinned dependency of this crate — see `traverse-framework/traverse` Spec 088 FR-001)
- any HTTP client, PostHog integration, or opt-in CLI command (all `traverse-cli`, not this crate)
- resolution failure handling beyond "do not emit an event" (no new error-reporting behavior)

## Requirements

### Functional Requirements

- **FR-001**: `crates/traverse-registry`'s public resolution API (the caret-range/semver resolution path used by `registry sync` and equivalent callers) MUST accept an optional parameter implementing the `UsageTelemetrySink` trait from `traverse-contracts` (once that crate publishes it per `traverse-framework/traverse` Spec 088). When absent (`None`/default), resolution behavior MUST be byte-for-byte identical to today's behavior — this crate MUST NOT construct a default network-capable sink itself.
- **FR-002**: On a successful version resolution, if a sink was supplied, this crate MUST invoke it with a `resolve` event carrying the resolved `namespace/id@version` and nothing else (no request metadata, no caller identity, no timing data beyond what the sink itself may choose to record).
- **FR-003**: On a failed or ambiguous resolution (no matching version, invalid range, etc.), this crate MUST NOT invoke the sink.
- **FR-004**: This crate MUST NOT depend on `traverse-cli`, any HTTP client crate, or any concrete `UsageTelemetrySink` implementation — only the trait definition from its existing `traverse-contracts` dependency. Adding such a dependency here would violate this crate's portability, which other embedders besides `traverse-cli` rely on.
- **FR-005**: This change MUST wait on `traverse-contracts` publishing the `UsageTelemetrySink` trait (`traverse-framework/traverse` Spec 088, implementation ticket `traverse-framework/traverse`#927) and this repo bumping its pinned `traverse-contracts` version (`Cargo.toml`'s `workspace.dependencies.traverse-contracts`) before implementation can start.
- **FR-006**: This spec MUST NOT be used to justify any other behavior change to the extracted crate beyond the resolve-hook described above, per `013-inherited-registry-governance` FR-002's own restriction on itself — a further behavior change needs its own future spec here, not an amendment to this one.

## Success Criteria

- **SC-001**: A successful resolution with a supplied sink invokes it exactly once with the correct `namespace/id@version`.
- **SC-002**: A failed resolution never invokes the sink, with or without one supplied.
- **SC-003**: Calling the resolution API with no sink supplied produces output byte-for-byte identical to the pre-existing behavior (no regression for any current caller that doesn't yet know about telemetry).
- **SC-004**: `cargo tree -p traverse-registry` shows no new dependency beyond the already-present, version-bumped `traverse-contracts`.

## Governing Relationship

This specification governs the resolve-hook behavior change to `crates/traverse-registry/` only. It is additive to, and does not replace or re-litigate, `013-inherited-registry-governance` or `014-extraction-compatibility`. The port trait itself, the CLI adapter, and the collector remain governed by `traverse-framework/traverse`'s Spec 088.

## Implementation Ticket

- `traverse-framework/registry`#144 — emit the `resolve` event via the port inside `crates/traverse-registry`'s resolution path (this repo's Project 3), blocked on `traverse-framework/traverse`#927 (port trait) publishing.
