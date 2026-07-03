# Feature Specification: Yank/Deprecation Mechanism

**Feature Branch**: `005-yank-deprecation`
**Created**: 2026-07-03
**Status**: Draft
**Input**: Registry ticket #8 — implement the deprecation ("yank") mechanism described in `001-registry-foundation` User Story 4 / FR-009.

## Purpose

Defines the exact deprecation-record file format and where exclusion logic lives, resolving the two open questions `001-registry-foundation` deferred for this ticket.

## Design Decisions

### Deprecation Record Format

A sibling file, `capabilities/<namespace>/<id>/<version>/deprecated.json`, added by a yank PR. The original `contract.json` in the same directory is never modified:

```json
{
  "deprecated": true,
  "reason": "short human-readable reason",
  "deprecated_at": "2026-07-03T00:00:00Z"
}
```

### Where Exclusion Logic Lives

Exclusion happens in **two places, by design**, matching the existing sync/publish split from `001-registry-foundation`:

1. **Index build** (this repo, spec `003-index-release-pipeline`): `index.json`'s per-capability `deprecated` field is set to `true` when a sibling `deprecated.json` exists. This is what most consumers (including anything reading `index.json` directly) see.
2. **Resolver** (`traverse-registry` crate, once extracted per ticket #9): range-based resolution (`^1.2.0`-style) MUST skip any capability version where `index.json`'s `deprecated` field is `true`. Exact-pin resolution ignores the flag entirely and always succeeds if the version exists.

This spec only governs (1), since (2) lives in a different repo's crate and depends on ticket #9 (crate extraction) landing first. This spec's CI-side validation work is independent of that dependency and can proceed now.

## Functional Requirements

- **FR-001**: A yank PR MUST add `deprecated.json` as a new file; it MUST NOT modify the existing `contract.json` in the same directory.
- **FR-002**: CI MUST reject a PR that modifies an existing `contract.json` (this overlaps with `002-capability-validation`'s namespace-collision/immutability checks — implemented once, referenced from both specs).
- **FR-003**: The index-build job (spec `003-index-release-pipeline`) MUST set `deprecated: true` in `index.json` for any capability version with a sibling `deprecated.json`.
- **FR-004**: `traverse-registry`'s resolver MUST exclude deprecated versions from range-based resolution while still resolving exact pins — tracked as follow-up work in the `traverse` repo once ticket #9 lands, not implemented in this repo.

## Success Criteria

- **SC-001**: Publishing `deprecated.json` for `traverse.example/1.2.0` never touches `contract.json` in that directory (verified by the PR diff containing only an addition, no modification).
- **SC-002**: The next `index.json` build after a yank PR merges shows `deprecated: true` for that version.
- **SC-003**: (Deferred to ticket #9 follow-up) A consumer resolving `^1.2.0` after the yank no longer receives that version; a consumer pinned exactly to `1.2.0` still does.

## Assumptions

- This spec's scope in this repo is the file format and the index-build reflection of it (FR-001 through FR-003). Actual resolver-side exclusion (FR-004) is explicitly out of scope until the crate extraction (ticket #9) lands — captured here so the file format is already correct by the time that work starts, avoiding a schema change later.
