# Feature Specification: Artifact Hosting & Release Convention

**Feature Branch**: `007-artifact-hosting`
**Created**: 2026-07-06
**Amended by**: registry#331 / registry#333 (artifact signature verification)
**Status**: Approved
**Input**: Registry issue #19 — resolve `docs/cross-repo-context.md` open question 3 (seed content) and the gap `001-registry-foundation` FR-007 left open: WASM artifacts live "as GitHub Release assets," but no spec says *which* releases, under what tags, uploaded when, by whom. Full reasoning: `docs/decision-log.md` entry 22. Amended per `docs/decision-log.md` entry 74 (registry#331) — `traverse-runtime` requires a real cryptographic signature, not just a digest, to execute a `PublishedGoverned` capability; this repo has never produced one.

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

## Amendment (registry#331 / registry#333): Artifact Signature Verification

### Why this exists

Digest verification (FR-001/FR-002 above) proves an artifact's *integrity* — the bytes fetched are the bytes that were published. It does not prove *authenticity* — that this repo's own governed publish process actually produced them. `traverse-runtime`'s execution-time trust model (`crates/traverse-runtime/src/security.rs`, `traverse-framework/traverse`) treats these as genuinely different guarantees: any capability resolved from a path this repo's own `specs/governance/approved-specs.json` `governs` (which includes `capabilities/`) is classified `ArtifactTrustLevel::PublishedGoverned`, and that trust tier requires a real cryptographic signature before execution, in every `RuntimeSecurityMode` — there is no development-mode bypass for it (only `ArtifactTrustLevel::LocalDev` gets one). This repo has never produced one; every `contract.json` published to date carries `artifact.digest`/`artifact.url` only. Full root-cause trace and the decisions below: `docs/decision-log.md` entry 74.

### Signature Scheme: Ed25519

`traverse-registry`'s own `ArtifactSignatureScheme` enum (`crates/traverse-registry/src/lib.rs` — this repo's own crate) supports `Ed25519` and `Sigstore`. This amendment adopts Ed25519: a single keypair signs every artifact, verification needs only the public key, and it requires no external transparency-log service — matching the same crypto and mental model this repo already uses elsewhere (the `traverse-cli serve` admin JWT is also Ed25519-signed, per traverse spec `033-http-json-api` FR-033/FR-034).

### Signature Field Shape: an Additive Sibling File, Never `contract.json`

Signing happens **after** a capability PR merges (see "Who Signs and When" below) — by which point `contract.json` is already immutable per this repo's core rule. The signature therefore can never be written into `contract.json`, for any capability, past or future; it is always a new, additive sibling file:

```
capabilities/<namespace>/<id>/<version>/signature.json
```

```json
{
  "scheme": "ed25519",
  "public_key_hex": "<hex-encoded Ed25519 public key>",
  "signature_hex": "<hex-encoded signature over the exact artifact bytes>",
  "sigstore_bundle_ref": null,
  "signed_at": "2026-08-28T00:00:00Z"
}
```

Field names deliberately mirror `traverse_registry::ArtifactSignature`'s own fields (`scheme`, `public_key_hex`, `signature_hex`, `sigstore_bundle_ref`) exactly, so a consumer's mapping from this file to that struct is a direct field-for-field copy, not a translation. `sigstore_bundle_ref` stays `null` under this scheme; it exists in the shape for forward compatibility if a future amendment ever adopts Sigstore for some or all artifacts. `signed_at` is additive metadata beyond what `ArtifactSignature` itself carries, for auditability.

This mirrors `specs/005-yank-deprecation`'s own established pattern (`deprecated.json` as a sibling of an immutable `contract.json`) — the same structural answer to the same underlying constraint.

### Who Signs and When

A CI job, triggered on merge to `main` (not on PR open or PR sync), detects newly-merged capability contracts, fetches the referenced artifact, signs the raw bytes with an Ed25519 private key held as a GitHub Actions secret, and commits the resulting `signature.json` sibling file. Signing only ever happens after a human has already approved the merge — the same trust boundary this repo already relies on for "only human approval actually gates merge" (`CLAUDE.md`) — and works identically for external contributors (e.g. Callweave) without them ever holding the signing key themselves.

### Public Key Publication

The public half of the signing key is published at a well-known, static path on this repo's existing catalog site (`https://registry.traverse-framework.com/signing-key.pub`), deployed by the same `deploy-catalog` CI job that already publishes `catalog.json`. This reuses infrastructure that is already this repo's documented public surface, rather than introducing a new one; a consuming host fetches and pins it once, the same trust-on-first-use model every mature package registry (npm, crates.io, etc.) already uses for its own signing keys.

### Backfill

All capabilities already published under this spec before this amendment — approximately 115 non-deprecated versions at the time of writing — get a `signature.json` sibling added in the same effort, using the same key and CI mechanism, run once rather than waiting for each to naturally get touched again. This is possible without a republish or version bump precisely because the signature is additive metadata about an existing, unchanged version, not an edit to its content — the same property that makes yank/deprecation additive. Deprecated capability versions are explicitly NOT backfilled: their data is already frozen by policy, and nothing needs a deprecated version to execute.

### Functional Requirements (Amendment)

- **FR-007**: Every non-deprecated capability version published under this spec MUST have a corresponding `capabilities/<namespace>/<id>/<version>/signature.json` sibling file once the CI signing job (FR-009) has run, with `scheme`, `public_key_hex`, `signature_hex`, and `signed_at` populated and `sigstore_bundle_ref` set to `null`.
- **FR-008**: `signature.json` MUST NOT be present for a version whose `contract.json` has no corresponding `artifact` field (workflow-backed capabilities, per the base spec's FR-001) — there is nothing to sign.
- **FR-009**: Signing MUST happen via CI, triggered only on merge to `main` (never on PR open/sync, never manually by an individual publisher holding the key) — signing a capability is gated on the same human-approval-then-merge event this repo already treats as its trust boundary.
- **FR-010**: The public verification key MUST be published at a stable, well-known URL on this repo's existing catalog site, kept current by the same deploy mechanism that already publishes `catalog.json`.
- **FR-011**: All non-deprecated capability versions published before this amendment landed MUST receive a `signature.json` backfill in the same effort as FR-009's CI job first shipping — this spec does not accept a permanently-partial signing state for historical content.
- **FR-012**: `signature.json`, once written for a given version, is immutable under the same rule as `contract.json` itself — never edited or replaced; a compromised or incorrect signature is corrected by rotating the signing key and re-running the backfill for affected versions, recorded as a decision-log entry, not by silently overwriting the file.

### Success Criteria (Amendment)

- **SC-004**: A freshly merged capability contract has a valid `signature.json` sibling within one CI run of merge, with no manual step required from the publisher.
- **SC-005**: Every one of this repo's ~115 pre-amendment, non-deprecated capability versions has a `signature.json` sibling after the backfill runs, verified by re-fetching each artifact and validating its signature against the published public key.
- **SC-006**: The public verification key is fetchable from the documented catalog URL and matches the key used to produce every currently-published `signature.json`.
- **SC-007**: A capability executed via a real `traverse-cli serve --registry-state --artifact-state` instance, once that side's own plumbing (tracked separately, `traverse-framework/traverse#1203`) can consume this signature, no longer fails with `artifact signature verification failed before execution` for `contractviolation` reasons tied to a missing signature.

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
- **(Amendment)** Building and running the CI signing job, and the one-time backfill script, are `registry#334`/`registry#335`'s own implementation scope — this amendment defines the convention (field shape, timing, key publication) those tickets implement against, not the scripts themselves.
- **(Amendment)** Consuming `signature.json` at execution time (extending `ArtifactStateEntry`, `registry materialize`, and `build_capability_registration_with_artifacts` to populate `BinaryReference.signature`) is `traverse-framework/traverse#1203`'s scope, tracked and evidenced there, not designed here — this repo does not control that repo's own crates.

## Approval

Drafted by an agent from a live `/brainstorm` session (registry#331, decision-log entry 74) that resolved every open design question one at a time with the repo owner, then given explicit, standalone authorization in the same conversation to both write and approve this amendment directly — a distinct instruction from the co-brainstorm itself, satisfying this repo's no-self-approval-of-specs rule the same way a separate confirmation always has (see `specs/019-public-metadata-sync-extension`'s and `specs/020-public-execution-proxy`'s own Approval sections for the general pattern this repo follows).

**Amendment approved 2026-08-28.** Version `1.0.0` → `1.1.0` (additive: new requirements layered on an unrelated-but-adjacent concern, no existing FR/SC changed or removed).
