# Feature Specification: Public-Scope Source of Truth & Publisher Identity

**Feature Branch**: `006-public-scope-and-identity`
**Created**: 2026-07-06
**Status**: Draft
**Input**: Registry issue #19 — resolve `docs/cross-repo-context.md` open question 1 (does this registry's scoping collide with the `traverse-registry` crate's bundle-level `scope: public/private`?). Companion normative spec: traverse-side, tracked by `traverse-framework/Traverse#548`. Full reasoning: `docs/decision-log.md` entries 20-21.

## Purpose

This spec defines this repo's side of the reconciliation between two scoping concepts that predate each other in different repos:

- **Bundle `scope: public/private`** (traverse spec `005-capability-registry` FR-006/FR-007; `RegistryScope`/`LookupScope` in the `traverse-registry` crate) — a **resolution-tier** axis: which tier of a local runtime's registry a record occupies, with private-overlay precedence on lookup.
- **This repo's `namespace`/`owner` fields** (spec `001-registry-foundation` FR-008) — a **publisher-identity** axis: who published a record and what naming scope it lives in.

The two axes are orthogonal and both remain. What this spec pins down is the meaning of "public": **the public tier of any Traverse runtime's local state is populated only by `traverse-cli registry sync` from this repo's published index. "Public" means "published in `traverse-framework/registry`," nothing else.** This follows the model every mature registry converged on (crates.io index + cargo path/`[patch]` overrides; npm scopes + `"private": true` publish latch; Go module proxy + `replace` directives): publicness is a fact about the source a record came from, never routing metadata an artifact author can declare.

The behavioral changes on the traverse side (sync-only public tier, publish-refusal latch for private bundles, rejection of local `scope: public` registration, shadow warnings) are normative in the traverse-side spec (`traverse-framework/Traverse#548`), not here. This spec governs what this repo itself asserts and validates: publisher identity, and the guarantee that this repo is a complete, self-sufficient source for the public capability universe.

This spec also finalizes the `owner`/`namespace` field shapes that `001-registry-foundation` reserved and `docs/decision-log.md` explicitly listed as deferred.

## User Scenarios & Testing

### User Story 1 - Resolve the Entire Public Universe From This Repo Alone (Priority: P1)

As a Traverse operator, I want the published index of this repo to be the complete public capability universe, so that after `registry sync` my runtime's public tier needs no other source and carries no ambiguity about where a public record came from.

**Why this priority**: This is the definitional core of the reconciliation — without a single source for "public," the same identity can enter the public tier from two places with different content, which the crate's immutability rule (`ImmutableVersionConflict`) turns into a hard failure at sync time.

**Independent Test**: Sync a fresh workspace, enumerate the public tier, and verify it equals exactly the contents of the latest `index-v*` release — nothing more, nothing less.

**Acceptance Scenarios**:

1. **Given** a published `index-v<N>` release, **When** a consumer syncs, **Then** every record in the local public tier is traceable to an entry in that `index.json` (namespace, id, version, digest).
2. **Given** a capability that was never published here, **When** a consumer inspects the public tier after sync, **Then** that capability is absent — regardless of any local bundle claiming `scope: public` (rejection of such registrations is enforced traverse-side per the companion spec).
3. **Given** this repo's index, **When** a consumer needs a public capability's artifact, **Then** `index.json`'s entry alone (digest + artifact URL) is sufficient to fetch and verify it, with no queries beyond the published release.

---

### User Story 2 - Attribute Every Published Record to a Publisher Identity (Priority: P1)

As a registry maintainer, I want every published record to carry finalized, validated `namespace` and `owner` fields, so that publisher identity is unambiguous now and third-party namespace claiming can be added later without a migration.

**Why this priority**: `001-registry-foundation` FR-008 reserved these fields precisely to avoid a breaking migration; leaving their shapes unfinalized re-creates that risk with every new publish.

**Independent Test**: Submit contracts with a missing `owner`, an `owner` of the wrong shape, and a `namespace` that doesn't match the path segment; verify each is rejected with a field-specific error.

**Acceptance Scenarios**:

1. **Given** a contract whose `namespace` string does not equal the `<namespace>` path segment, **When** CI validates, **Then** the PR is rejected (`contract.namespace_mismatch`).
2. **Given** a contract whose `owner` is missing or is not an object containing at least a `team` string, **When** CI validates, **Then** the PR is rejected with the specific field named.
3. **Given** a valid published record, **When** the index is built, **Then** the record's identity axes are fully determined by `namespace` + `id` + `version`, and `owner` is carried as attribution metadata — never as part of resolution identity.

---

### Edge Cases

- **A contract claims a resolution tier.** Records in this repo make no claim about `scope` — a `scope` field in a published `contract.json` is meaningless here (the tier is a consumer-side fact). Validation MUST reject a contract that includes a top-level `scope` field, to prevent the two axes from being silently conflated in published records.
- **Two sources race to define the same public identity.** Impossible by construction: the path-encoded layout (spec 001) means this repo cannot hold two records at one identity, and the companion spec removes the only other writer of the public tier (local public registration).
- **`owner` object shape drifts from `traverse-contracts`.** The `owner` shape here is defined as "the `traverse-contracts` `Owner` shape" by reference, not a local re-definition — if that crate's schema evolves, this repo follows it under the inherited compatibility policy rather than forking.

## Requirements

### Functional Requirements

- **FR-001**: The published index of this repo MUST be a complete description of the public capability universe: for every published, non-yanked capability version, `index.json` carries namespace, id, version, digest, artifact URL, and deprecated flag sufficient for resolution and fetch with no further queries (restating spec 003 FR-004 as a completeness guarantee, not just a format rule).
- **FR-002**: `namespace` MUST be a non-empty string exactly equal to the `<namespace>` path segment. It is a publisher-scoped naming prefix (the npm-scope analog), not a resolution tier.
- **FR-003**: `owner` MUST be an object conforming to the `traverse-contracts` `Owner` shape, with at least a non-empty `team` string; `contact` is optional metadata. This finalizes — and supersedes — spec 002's prose description of `owner` as a string, which the validator never enforced and published example contracts never used.
- **FR-004**: A published `contract.json` MUST NOT contain a top-level `scope` field. Resolution tier is a consumer-side concept; this repo publishes identity and content only.
- **FR-005**: `owner` MUST NOT participate in resolution identity. Identity is `namespace` + `id` + `version` alone; two records differing only in `owner` at the same identity are the same (structurally impossible) record.
- **FR-006**: This repo MUST NOT define or enforce the consumer-side behaviors of the reconciliation (sync-only public tier population, private publish latch, local-public registration rejection, shadow warnings) — those are normative in the traverse-side companion spec, and this repo's docs MUST reference that spec rather than restate its rules.

### Key Entities

- **Publisher identity**: the (`owner`, `namespace`) pair attributing a published record — who stands behind it and what naming scope it occupies.
- **Public tier** (defined traverse-side, referenced here): the portion of a runtime's local durable registry state populated exclusively by `registry sync` from this repo's index.
- **Companion spec**: the traverse-side normative spec produced under `traverse-framework/Traverse#548`, which owns bundle `scope` semantics, dual-mode manifest resolution, and sync/registration behavior.

## Success Criteria

- **SC-001**: A fresh sync followed by a full enumeration of the public tier matches the latest `index.json` exactly (no extra records, no missing records).
- **SC-002**: A contract PR with a top-level `scope` field, a mis-shaped `owner`, or a path-mismatched `namespace` is rejected pre-merge with a field-specific error.
- **SC-003**: The org's documentation contains exactly one definition of "public" (this spec + its traverse companion), and `traverse`'s `registry-bundle-authoring-guide.md` scope row is updated to reference it once the companion spec lands.

## Assumptions

- Team-shared private registries — multiple sync sources with per-namespace routing, npm-`.npmrc`-style — are anticipated and deliberately deferred. This spec's single-source rule is written against "the public tier," so generalizing to a configured source list later is additive: each additional source populates its own named tier without changing what this repo asserts.
- The only accepted publisher remains the core team via PR review (spec 001's assumption stands); `namespace` claiming and identity verification for third parties remain deferred, and FR-002/FR-003 are the fields that future work will build on.
- The traverse-side companion spec (`traverse-framework/Traverse#548`) is authored and approved in the traverse repo before any traverse-side behavior described here is implemented; if its design diverges from this spec's description of the consumer side, the two specs must be reconciled before either proceeds — neither silently wins.
