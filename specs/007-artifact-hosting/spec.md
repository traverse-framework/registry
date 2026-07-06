# Feature Specification: Artifact Hosting & Release Convention

**Feature Branch**: `007-artifact-hosting`
**Created**: 2026-07-06
**Status**: Approved
**Input**: Registry issue #19 — resolve `docs/cross-repo-context.md` open question 3 (seed content) and the gap `001-registry-foundation` FR-007 left open: WASM artifacts live "as GitHub Release assets," but no spec says *which* releases, under what tags, uploaded when, by whom. Full reasoning: `docs/decision-log.md` entry 22.

## Purpose

This spec defines the concrete artifact-hosting convention for capabilities published in this repo, and the manual publish runbook that uses it — including the registry's first real content, `traverse-starter.process` 1.0.0.

`001-registry-foundation` FR-007 forbids committing WASM binaries into git and requires digest + URL references to GitHub Release assets. Spec 002 requires CI to fetch the referenced asset and verify its SHA-256 digest before merge. Neither says where the asset lives or how it gets there before the contract PR exists. This spec closes that loop:

- artifacts are hosted as Release assets **in this repo**, under a deterministic tag scheme
- the artifact release is created **before** the contract PR is opened, so digest verification has something real to fetch
- artifact releases are immutable once referenced by a merged contract

Artifact hosting lives in this repo (not in `traverse` or a publisher's repo) because this repo owns the immutability guarantee: a published record's integrity must not depend on another repository's release hygiene.

## Design Decisions

### Release Tag Scheme

One release per capability version's artifact set, tagged:

```
artifacts/<namespace>.<id>-<version>
```

Example: `artifacts/traverse-starter.traverse-starter.process-1.0.0` would be redundant — since `<id>` values in this org already embed the namespace prefix (e.g. `traverse-starter.process`), the tag uses the `<id>` and `<version>` alone:

```
artifacts/<id>-<version>          e.g. artifacts/traverse-starter.process-1.0.0
```

This is unambiguous because `id` is globally unique across the registry (path layout: one `<id>` directory per `<namespace>`, and ids embed their namespace by existing convention). If a future id ever fails to embed its namespace, the tag falls back to `artifacts/<namespace>.<id>-<version>`; the contract's `artifact.url` is always the authoritative pointer either way — consumers never construct tag names.

### Artifact Reference Field

A WASM-backed capability's published `contract.json` MUST include:

```json
"artifact": {
  "digest": "sha256:<hex>",
  "url": "https://github.com/traverse-framework/registry/releases/download/artifacts/<id>-<version>/<asset-name>"
}
```

This is the `{digest, url}` shape `scripts/ci/capability_validation.py` already validates when present; this spec makes it required for WASM-backed capabilities rather than optional. Capabilities whose implementation is a workflow reference (composed capabilities, per traverse spec 005 FR-015) have no binary artifact and omit the field.

The published registry copy of a contract is the publication record — adding the `artifact` field here does not require touching the source contract in the author's repo.

### Upload-Before-Publish Flow

Ordering is load-bearing: spec 002's digest check fetches the asset during PR validation, so the asset must exist first.

1. Build the WASM artifact; compute `sha256` of the exact bytes.
2. Create the `artifacts/<id>-<version>` release in this repo and upload the asset.
3. Open the contract PR with `artifact.digest` + `artifact.url` pointing at the uploaded asset.
4. CI fetches the asset, verifies the digest, runs all other deterministic checks.
5. Human review + merge → index build picks up digest + URL into `index.json` (spec 003).

### Immutability

Once a contract referencing an `artifacts/*` release merges to `main`, that release and its assets are immutable: assets are never replaced, renamed, or deleted, and the release is never re-tagged. A bad artifact is corrected the same way as a bad contract — publish a new version and yank the old one (spec 005). An `artifacts/*` release whose contract PR was never merged (abandoned publish) MAY be deleted.

`artifacts/*` releases are disjoint from `index-v*` releases (spec 003): index releases carry the registry state; artifact releases carry capability binaries. Neither ever contains the other's assets.

### Manual Publish Runbook (until traverse #543 automates it)

The maintainer flow for a publish, exercised first by the seed:

1. Obtain the built artifact and its digest (for the seed: `process-agent.wasm` from `traverse`'s `examples/traverse-starter/process-agent/artifacts/`, digest `sha256:5647c39a1d25d8728350f9619025292a62e78a602068a2ad9b6f075751c93d99` — already recorded in the reference app's component manifest).
2. `gh release create artifacts/<id>-<version> <asset> --repo traverse-framework/registry --title "<id> <version> artifacts" --notes "<one-liner>"`.
3. Copy the source `contract.json` to `capabilities/<namespace>/<id>/<version>/contract.json`, adding the `artifact` field.
4. Open the PR (Governing Spec: this spec + 001), let CI verify, human-approve, merge.
5. Confirm the new `index-v<N>` release lists the capability with the correct digest and URL.

`traverse-cli capability publish` (traverse #543) later automates steps 2-4 without changing their semantics.

## User Scenarios & Testing

### User Story 1 - Publish the Seed Capability End to End (Priority: P1)

As the registry maintainer, I want to publish `traverse-starter.process` 1.0.0 — artifact release, contract PR, digest verification, index release — so that the entire pipeline is proven on real content and downstream work (traverse #542/#543, reference-apps adoption) has something real to build against.

**Why this priority**: the registry has been fully implemented and completely empty; nothing else in the org can currently prove the pipeline works end to end.

**Independent Test**: Execute the runbook above for the seed; verify the resulting `index.json` entry resolves and its artifact fetches + digest-verifies from a clean machine.

**Acceptance Scenarios**:

1. **Given** the artifact release exists with the correct asset, **When** the contract PR runs CI, **Then** digest verification fetches the asset from this repo's release and passes.
2. **Given** the merged seed, **When** the index builds, **Then** `index.json` lists `traverse-starter` / `traverse-starter.process` / `1.0.0` with `deprecated: false`, the recorded digest, and an `artifact_url` under this repo's `artifacts/` release.
3. **Given** a contract PR whose `artifact.url` points at a nonexistent asset or whose digest mismatches, **When** CI runs, **Then** the PR is rejected before merge.

---

### User Story 2 - Correct a Bad Artifact Without Breaking Immutability (Priority: P2)

As a registry maintainer, I want a defined correction path for a bad published artifact, so that immutability never turns a mistake into a permanent hazard.

**Independent Test**: Simulate a bad publish; verify the correction path is publish-new-version + yank-old, and that the old release's assets remain untouched and exact-pin-resolvable.

**Acceptance Scenarios**:

1. **Given** a merged contract referencing an `artifacts/*` release, **When** any change to that release's assets is proposed, **Then** it is refused — the correction path is a new version plus a yank record (spec 005).
2. **Given** a yanked version, **When** a consumer resolves it by exact pin, **Then** its artifact still fetches and digest-verifies (yank never deletes assets).

---

### Edge Cases

- **Race between asset upload and PR validation**: if the release exists but the asset upload is incomplete when CI fetches, digest verification fails and the PR re-runs after upload completes — safe failure, no partial publish.
- **Same artifact bytes shared by multiple capability versions**: allowed; each version's release carries its own copy (storage is cheap; sharing assets across releases would couple their immutability lifetimes).
- **Abandoned publish**: an `artifacts/*` release whose contract never merged may be deleted; nothing published references it.

## Requirements

### Functional Requirements

- **FR-001**: Every WASM-backed capability published in this repo MUST include an `artifact` field with `digest` (`sha256:`-prefixed) and `url`; workflow-backed capabilities omit it.
- **FR-002**: Artifact assets for capabilities published here MUST be hosted as GitHub Release assets in this repo under the `artifacts/<id>-<version>` tag scheme (falling back to `artifacts/<namespace>.<id>-<version>` if an id does not embed its namespace).
- **FR-003**: The artifact release MUST exist, with its assets fully uploaded, before the referencing contract PR can pass validation (upload-before-publish).
- **FR-004**: Once a referencing contract merges to `main`, the `artifacts/*` release and its assets MUST be treated as immutable — never replaced, renamed, deleted, or re-tagged; corrections go through publish-new-version + yank (spec 005).
- **FR-005**: `artifacts/*` releases and `index-v*` releases MUST remain disjoint: index releases never carry capability binaries; artifact releases never carry `index.json`.
- **FR-006**: The first published content under this spec MUST be `traverse-starter.process` 1.0.0, published as-is (namespace `traverse-starter`, existing owner object, digest `sha256:5647c39a...`) via the manual runbook — deliberately without the #543 CLI, proving the pipeline is CLI-independent.

## Success Criteria

- **SC-001**: The seed publish completes with all deterministic checks passing, exactly one human approval, and an `index-v<N>` release listing the capability.
- **SC-002**: From a machine with no prior state, the seed's `index.json` entry alone is sufficient to fetch the artifact and verify its digest.
- **SC-003**: No file under `capabilities/` and no asset under any referenced `artifacts/*` release is ever modified after merge — verified structurally (new files/releases only).

## Assumptions

- The seed's WASM artifact is the one already built and digest-pinned in `reference-apps`' component manifest; no rebuild is needed for the seed publish. If the artifact is ever rebuilt, its digest changes and it is a different publish.
- GitHub Releases remain free and adequate for artifact volume at this stage (spec 001's cost-deferral assumption); the future hosted layer (decision log entry 4) would serve the same digests from object storage without a schema change, since consumers only ever follow `artifact.url` + verify `artifact.digest`.
- `traverse-cli capability publish` (traverse #543) will automate the runbook's mechanical steps without changing the tag scheme, field shapes, or ordering defined here.
