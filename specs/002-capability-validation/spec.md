# Feature Specification: Capability Validation Gate

**Feature Branch**: `002-capability-validation`
**Created**: 2026-07-03
**Status**: Approved
**Input**: Registry ticket #5 — implement the deterministic CI validation job described in `001-registry-foundation` FR-003, replacing the placeholder JSON-well-formedness-only check.

## Purpose

This spec defines the exact, implementable rules for the deterministic capability-validation CI job: schema validation, semver-bump-vs-actual-diff classification, artifact digest integrity, namespace collision detection, and dependency resolvability. It resolves the design questions `001-registry-foundation` left open for this ticket.

## Design Decisions

### Schema

A `contract.json` at `capabilities/<namespace>/<id>/<version>/contract.json` MUST validate against the capability contract schema inherited from `traverse-framework/traverse`'s `traverse-contracts` crate (spec `002-capability-contracts`), plus these registry-specific fields:

- `owner` (string, required) — defaults to `"core"` for every capability published under this spec's scope (per `001-registry-foundation` FR-008).
- `namespace` (string, required) — MUST match the `<namespace>` path segment exactly. Defaults to `"core"`.
- `id` (string, required) — MUST match the `<id>` path segment exactly.
- `version` (string, required) — MUST be a valid semver string and MUST match the `<version>` path segment exactly.

### Semver-Diff Classification

CI compares the new contract against the immediately preceding published version of the same `<namespace>/<id>` (if any) and classifies the change:

- **MAJOR**: a required field was removed; input/output meaning changed incompatibly; required events, permissions, or constraints changed incompatibly (per the inherited `docs/compatibility-policy.md` breaking-change rules).
- **MINOR**: a new optional field was added; additive metadata was added that doesn't change required behavior.
- **PATCH**: description/documentation-only changes with no semantic effect.

CI rejects the PR if the declared version bump is smaller than the detected change class (e.g. a MAJOR-class change published as a PATCH bump).

### Digest Integrity

If `contract.json` references an artifact (WASM binary or source), the referenced digest MUST match the actual content at the referenced GitHub Release asset URL, computed via SHA-256. CI fetches the asset and verifies the digest before allowing merge.

### Namespace Collision

CI rejects a PR if it attempts to create a `contract.json` at a `<namespace>/<id>/<version>` path that already exists on `main` (this is also structurally prevented by git's own merge-conflict behavior on the same file path, per `001-registry-foundation`'s Edge Cases, but CI makes the rejection explicit and readable rather than relying solely on a raw git conflict).

### Dependency Resolvability

If `contract.json` declares a `dependencies` field (inherited shape from `traverse`'s spec `043-module-dependency-management`), CI verifies each declared `capability_id`/`version_range` pair resolves against capabilities already published in this repo. Unresolvable dependencies fail with the same `dependency_unsatisfiable` error shape spec 043 already defines, for consistency with the inherited model.

## Functional Requirements

- **FR-001**: CI MUST validate `contract.json` against the schema described above and fail with a field-specific error on violation.
- **FR-002**: CI MUST classify the semver diff and fail if the declared bump is smaller than the detected class.
- **FR-003**: CI MUST verify artifact digest integrity for any referenced artifact.
- **FR-004**: CI MUST reject a PR that collides with an existing published path.
- **FR-005**: CI MUST verify declared dependencies are resolvable against already-published capabilities.
- **FR-006**: All five checks MUST run as part of the existing `spec-alignment`-adjacent CI job, replacing the current placeholder `capability-structure-check` job in `.github/workflows/ci.yml`.

## Success Criteria

- **SC-001**: A capability contract with a missing required field is rejected with the specific missing field named in the error.
- **SC-002**: A MAJOR-class change declared as a PATCH bump is rejected before merge.
- **SC-003**: A tampered artifact (digest mismatch) is rejected before merge.
- **SC-004**: Two PRs racing to publish the same path cannot both merge — the second is rejected either by this check or by git's own conflict behavior.

## Assumptions

- This validation runs as a GitHub Actions job with network access to fetch Release assets for digest verification.
- Full implementation depends on the `traverse-contracts` crate being consumable from this repo (as a published dependency, or via a lightweight schema-only re-implementation) — see Open Question below.

## Open Question For Implementation

Should this repo depend on the published `traverse-contracts` crate directly (requiring a Rust toolchain in CI), or re-implement schema validation in the CI script's existing Python tooling (lighter weight, but risks schema drift from the canonical Rust implementation)? Recommendation: depend on `traverse-contracts` directly once it's published to crates.io, for a single source of truth — track as a follow-up decision when this spec is approved for implementation.
