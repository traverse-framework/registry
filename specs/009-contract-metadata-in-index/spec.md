# Feature Specification: Contract Metadata in the Public Release Index

**Feature Branch**: `claude/issue-44-contract-metadata-index`
**Created**: 2026-07-14
**Status**: Draft
**Input**: Registry ticket #44 — publish immutable capability-contract metadata
in every public Registry release index, per `traverse-framework/traverse`
spec `063-registry-contract-materialization` FR-001 and FR-002.

## Purpose

`specs/003-index-release-pipeline` defined `index.json`'s shape with
artifact-only provenance (`digest`, `artifact_url`). Traverse's consumer-side
`registry_ref` materialization path (Traverse #551) needs to register a
capability from a verified **contract**, not just a verified WASM artifact —
an artifact-only record lets a consumer register a capability whose
behavioral contract was never fetched or checked. This spec adds contract
provenance fields to the existing index record and defines the rejection
behavior when they cannot be produced.

This is additive to `specs/003-index-release-pipeline`, which stays
unmodified (immutable) — this spec amends `scripts/ci/build_index.py`'s
behavior beyond what 003 described.

## Design Decisions

### New Index Record Fields

Each entry in `index.json`'s `capabilities` array gains two fields:

```json
{
  "namespace": "core",
  "id": "example-capability",
  "version": "1.0.0",
  "digest": "sha256:...",
  "artifact_url": "https://github.com/.../example-capability-1.0.0.wasm",
  "contract_digest": "sha256:...",
  "contract_url": "https://raw.githubusercontent.com/traverse-framework/registry/<source_commit>/capabilities/core/example-capability/1.0.0/contract.json",
  "deprecated": false
}
```

- `contract_digest`: `sha256:<hex>` of the exact bytes of the published
  `contract.json` file, computed at index-build time by reading the file
  directly (the "fetch" is a local read of the same immutable, merged file
  the index is being built from — there is no separate network hop to
  verify against at build time, since the source of truth and the digest
  input are the same bytes by construction).
- `contract_url`: a `raw.githubusercontent.com` URL pinned to the current
  build's `source_commit` SHA (the same commit already used for
  `index.json`'s top-level `source_commit`). Because
  `capabilities/<namespace>/<id>/<version>/contract.json` is structurally
  immutable (decision-log 10: a new version is always a new path, an
  existing path is never edited — enforced by `capability_validation.py`'s
  immutability check), any commit SHA on `main` at or after the file's
  introduction resolves to byte-identical content. Pinning to the *current*
  build's commit (rather than resolving the file's original introducing
  commit) is sufficient for immutability and avoids an extra `git log`
  lookup per record.

### Rejection Behavior

`build_index.py` MUST fail the build (non-zero exit, no `index.json`
written) rather than silently skip a record when a `capabilities/**/contract.json`
file exists but cannot be read or parsed. The previous behavior
(`except Exception: continue`) silently dropped bad records from the public
index, which is exactly the "artifact-only fallback" failure mode this spec
exists to close off. Each failure is reported with a stable error code
(`index.contract_unreadable`) and the offending path, mirroring the error
shape already used by `capability_validation.py`.

This only covers *build-time* generation — a record already published in a
past `index.json` release is never retracted by this rule (retraction is
exclusively the existing yank mechanism, `specs/005-yank-deprecation`).

### Yank Interaction

Unchanged from `specs/003-index-release-pipeline` / `specs/005-yank-deprecation`:
a yanked version still gets a `capabilities` entry (with `deprecated: true`,
including its `contract_digest`/`contract_url`) so an exact pin can still
resolve it; only range-based resolution excludes it, and that exclusion
happens consumer-side, not in the index.

## Functional Requirements

- **FR-001**: `index.json` entries MUST include `contract_digest`
  (`sha256:<hex>` of the published `contract.json` bytes) and `contract_url`
  (a commit-pinned `raw.githubusercontent.com` URL to that same file).
- **FR-002**: `contract_digest` and `contract_url` MUST be derived from the
  same build input (the current commit's `capabilities/` tree) as the
  existing `digest`/`artifact_url` fields, so all four fields describe one
  consistent release state.
- **FR-003**: `build_index.py` MUST fail the build with a stable,
  actionable error (`index.contract_unreadable`, including the file path)
  when a discovered `contract.json` cannot be read or parsed, instead of
  omitting it from the index.
- **FR-004**: Yanked/deprecated versions MUST retain `contract_digest` and
  `contract_url` in their index entry (consistent with existing
  `digest`/`artifact_url` handling) — only resolution-time exclusion
  differs, not index inclusion.

## Success Criteria

- **SC-001**: Every entry in a freshly built `index.json` has a non-null
  `contract_digest` and `contract_url`.
- **SC-002**: A `contract.json` that fails to parse aborts the index build
  with `index.contract_unreadable` rather than producing a shorter index.
- **SC-003**: `contract_digest` recomputed independently from the file at
  `contract_url` matches the published `contract_digest` (verified by test
  fixture, not a live network fetch in CI).
- **SC-004**: A yanked capability version's entry still carries
  `contract_digest`/`contract_url` in the same build that marks it
  `deprecated: true`.

## Assumptions

- This spec does not change `specs/003-index-release-pipeline`'s versioning
  or publish mechanism — only the record shape `build_index.py` emits.
- Traverse-side consumption (Traverse #551 registering via `registry_ref`
  with contract fetch + digest verification, and Traverse #552's
  bundle-scope enforcement) is out of this repo's scope
  (registry-scope-only) and is not implemented here.

## Approval

Drafted by an agent from registry ticket #44's Definition of Done, which
itself derives from the already-approved `traverse-framework/traverse` spec
`063-registry-contract-materialization`. Per this repo's no-self-approval
rule (`AGENTS.md`), this spec stays `Draft` — moving it to `Approved` in
`specs/governance/approved-specs.json` requires the repo owner's explicit,
standalone sign-off.
