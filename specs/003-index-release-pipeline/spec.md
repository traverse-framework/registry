# Feature Specification: Index/Release Publishing Pipeline

**Feature Branch**: `003-index-release-pipeline`
**Created**: 2026-07-03
**Status**: Approved
**Input**: Registry ticket #6 — implement the CI job described in `001-registry-foundation` FR-006: build a versioned aggregated index artifact on merge to `main` and publish it as a GitHub Release.

## Purpose

Defines the concrete index artifact format and versioning scheme so `traverse-cli registry sync` has something well-defined to fetch.

## Design Decisions

### Index Artifact Format

A single JSON file, `index.json`, generated on every merge to `main` by walking `capabilities/**/contract.json`:

```json
{
  "index_version": 42,
  "generated_at": "2026-07-03T00:00:00Z",
  "source_commit": "<git sha of main at build time>",
  "capabilities": [
    {
      "namespace": "core",
      "id": "example-capability",
      "version": "1.0.0",
      "digest": "sha256:...",
      "artifact_url": "https://github.com/traverse-framework/registry/releases/download/index-v42/example-capability-1.0.0.wasm",
      "deprecated": false
    }
  ]
}
```

### Versioning Scheme

`index_version` is a monotonically increasing integer, incremented by exactly 1 on every successful merge to `main` that changes `capabilities/`. It is **not** the same as any individual capability's semver — it's a version for "the state of the whole registry," analogous to a database migration counter.

### Publish Mechanism

On merge to `main`:

1. CI walks `capabilities/**/contract.json`, builds `index.json` as above.
2. CI reads the previous `index_version` from the latest GitHub Release tagged `index-v<N>` (or starts at 1 if none exists).
3. CI creates a new GitHub Release tagged `index-v<N+1>`, uploading `index.json` as a release asset.
4. The job is idempotent: if a release for the target tag already exists (re-run scenario), it updates the existing release's asset rather than failing.

### Sync Consumption

`traverse-cli registry sync` (implemented in `traverse-framework/traverse`) fetches the `index.json` asset from the latest `index-v*` GitHub Release via the GitHub Releases API (unauthenticated, since this is a public repo) and writes it to local durable workspace state.

## Functional Requirements

- **FR-001**: CI MUST generate `index.json` from the current state of `capabilities/` on every merge to `main`.
- **FR-002**: CI MUST publish `index.json` as a GitHub Release asset tagged `index-v<N>`, where `N` increments monotonically.
- **FR-003**: The publish job MUST be idempotent — re-running it for an already-published `index_version` must not fail or create a duplicate release.
- **FR-004**: `index.json` MUST include enough information (namespace, id, version, digest, artifact URL, deprecated flag) for a consumer to resolve any published capability without additional queries to this repo.

## Success Criteria

- **SC-001**: After merging a PR that adds a new capability, a new GitHub Release with an incremented `index_version` exists within the CI run's completion.
- **SC-002**: `index.json`'s `capabilities` array includes every currently-published, non-deleted capability version.
- **SC-003**: Re-running the publish job for the same commit does not create duplicate releases or corrupt the existing one.

## Assumptions

- GitHub Releases and Actions remain free for this public repo at the volume this registry will see for the foreseeable future (per `001-registry-foundation`'s cost-deferral assumption).
- This spec does not cover the future hosted-API layer — `index.json`'s schema is kept simple and storage-agnostic specifically so a hosted API can later serve equivalent data without a breaking change to consumers.
