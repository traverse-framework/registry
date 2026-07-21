# Feature Specification: Inherited Registry Governance

**Feature Branch**: `013-inherited-registry-governance`
**Created**: 2026-07-18
**Status**: Approved
**Input**: Blanket governance for `crates/traverse-registry/` content whose original requirements live in `traverse-framework/traverse` specs that also govern other crates staying in `traverse` (so those specs cannot be re-adopted wholesale the way `011-capability-registry-adoption` and `012-event-registry-adoption` were). Decided via `/brainstorm` with the repo owner, 2026-07-18 -- the "hybrid" option in that session: copy the two specs that were already registry-exclusive, blanket-cover everything else rather than attempt per-FR extraction across 19 multi-crate specs. Full reasoning: `docs/decision-log.md` entry 29.

**Enforcement note**: `crates/traverse-registry/` now physically exists in this repo (the `010-crate-publish-pipeline` scaffold, #52), so this spec registers directly in `specs/governance/approved-specs.json` alongside `011`/`012` rather than waiting -- decision log entry 26(f)'s concern was specifically about declaring governance over a path that doesn't exist yet; that no longer applies.

**Amendment (2026-07-21, licensed by this spec's own FR-003)**: a full audit of the crate's compiled source, done while writing `014-extraction-compatibility`, found `002-capability-contracts` also governs `crates/traverse-registry/` -- a 20th spec, missed in the original 2026-07-18 audit of 19. Added to the source-specs table below per FR-003's explicit re-verification requirement. Full reasoning: `docs/decision-log.md` entry 33.

## Purpose

Nineteen `traverse` specs govern `crates/traverse-registry/` alongside at least one other crate that is **not** moving (`traverse-runtime`, `traverse-cli`, or `traverse-contracts`). None of them is primarily "about" the registry in the way `005-capability-registry` and `011-event-registry` were -- each describes behavior that spans the registry and its consumers together, and splitting out only the registry-relevant requirements from each would require real synthesis work with no clean seams (several FRs describe a registry/runtime or registry/CLI interaction as a single requirement).

Rather than either (a) copying and re-scoping all 19, which would cost significant authoring effort and create nineteen more permanently-diverging document pairs, or (b) leaving `crates/traverse-registry/`'s inherited behavior ungoverned here, this spec makes an explicit, single governing statement: **the crate's behavior, as extracted, is governed as-is by its state at the moment of extraction** -- traceable to the specs listed below for original context and rationale, without restating their requirements here.

This spec deliberately does not restate functional requirements from the sources below. That is the entire point of the "blanket" option chosen over full re-adoption: it accepts losing fine-grained, spec-numbered traceability for the crate's pre-existing behavior in exchange for zero duplication and no ongoing two-repo drift risk on content that was never meant to be registry-exclusive.

## Scope

Governs `crates/traverse-registry/` for any behavior not already covered by `011-capability-registry-adoption` or `012-event-registry-adoption`.

Does **not** govern the corresponding behavior in `traverse-runtime`, `traverse-cli`, or `traverse-contracts` -- those crates stay in `traverse`, governed by `traverse`'s own (now-trimmed) copies of the source specs below.

## Source Specs (Inherited As-Is, Full Context Lives in `traverse`)

At the time of extraction, the following `traverse-framework/traverse` specs governed `crates/traverse-registry/` alongside at least one other crate. Their registry-relevant requirements are inherited into this crate as-is; consult the named spec in `traverse-framework/traverse`'s `specs/` directory for full FR-level detail and rationale:

| Spec | Title |
|---|---|
| `002-capability-contracts` | Capability Contracts (added 2026-07-21, see Amendment above) |
| `007-workflow-registry-traversal` | Workflow Registry and Deterministic Traversal |
| `018-event-driven-composition` | Event-Driven Composition |
| `034-programmatic-registration` | Programmatic Registration API |
| `035-multi-agent-isolation` | Workspace Auth and Multi-Agent Isolation |
| `036-event-subscription-replay` | Event Subscription and Replay |
| `037-semver-range-resolution` | Semver Range Resolution |
| `039-connector-plugin-architecture` | Connector Plugin Architecture |
| `041-workflow-composition-api` | Workflow Composition API |
| `043-module-dependency-management` | Module Dependency Management |
| `044-application-bundle-manifest` | Application Bundle Manifest |
| `045-governed-model-dependency-resolution` | Governed Model Dependency Resolution |
| `046-public-cli-app-registration` | Public CLI App Registration |
| `048-semver-publishing-pipeline` | Semver Publishing Pipeline (governs `crates/traverse-registry/Cargo.toml` specifically) |
| `052-app-state-machine` | App State Machine |
| `053-conditional-state-transitions` | Conditional State Transitions |
| `054-public-scope-registry-ref` | Public Scope and Registry References |
| `055-registry-sync` | Registry Sync CLI |
| `058-workflow-pipeline-execution` | Workflow Pipeline Execution |
| `063-registry-contract-materialization` | Registry Contract Materialization |

This list is a snapshot as of 2026-07-18 (`traverse` decision log / `traverse#627` DoD). It MUST be re-verified against `traverse`'s current `specs/governance/approved-specs.json` before this spec's `approved-specs.json` entry is registered, since new specs may have been approved there in the interim that also touch `crates/traverse-registry/`.

## Requirements

### Functional Requirements

- **FR-001**: `crates/traverse-registry/`'s behavior, as physically extracted from `traverse`, MUST be treated as governed and correct as-is -- extraction is a location change, not a behavior change, and requires no new implementation work to satisfy this spec.
- **FR-002**: This spec MUST NOT be used to justify behavior changes to the extracted crate; any new requirement or behavior change from this point forward MUST go through a new, dedicated spec in this repo (this repo already has its own spec numbering for exactly that -- see `001-registry-foundation` through `012-event-registry-adoption`), not an edit to this spec or the specs it inherits from.
- **FR-003**: The source-specs table above MUST be re-verified for completeness against `traverse`'s current governance registry immediately before this spec's `approved-specs.json` entry is registered, and updated if new specs have started governing `crates/traverse-registry/` since 2026-07-18.
- **FR-004**: Any future change to the extracted crate that would otherwise fall under one of the source specs' original intent MUST be evaluated against a new spec here, not assumed to remain governed by the inherited, frozen source-spec list.

## Success Criteria

- **SC-001**: `traverse#627`'s crate move requires zero new registry-side spec authoring beyond `011`, `012`, and this spec -- the 19 multi-crate specs above are not individually re-adopted.
- **SC-002**: No behavior change to `crates/traverse-registry/` ships under cover of this spec alone; every behavior change after extraction traces to its own dedicated spec.
- **SC-003**: `traverse`'s own copies of the 19 source specs remain fully valid governance for their still-relevant crates (`traverse-runtime`, `traverse-cli`, `traverse-contracts`) after their `crates/traverse-registry/` path is trimmed from `governs`.

## Governing Relationship

This specification governs `crates/traverse-registry/` in this repo for all content not covered by `011-capability-registry-adoption` or `012-event-registry-adoption`.
