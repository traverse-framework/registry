# Feature Specification: Registry Foundation

**Feature Branch**: `001-registry-foundation`
**Created**: 2026-07-03
**Status**: Draft
**Input**: Brainstorm session establishing `traverse-framework/registry` as the public capability registry for Traverse, extracted from `traverse-framework/traverse`'s `crates/traverse-registry`. Full narrative record: `docs/decision-log.md`. Companion migration record: `traverse-framework/traverse` `specs/051-registry-extraction/spec.md`.

## Purpose

This specification defines the first governing slice for `traverse-framework/registry`: a git-based, CI-validated, PR-published registry of Traverse capability contracts and their artifacts.

This repo has no prior context of its own — it did not exist before this spec. Everything a contributor or an AI agent needs to understand this repo's scope starts here.

This spec governs:

- the repository layout for capability records
- the publish mechanism (PR merge → CI-built versioned index artifact)
- the quality gate (deterministic checks + advisory AI review)
- the deprecation/yank mechanism
- reserved schema fields for future third-party publishing
- artifact storage for WASM binaries

This spec does **not** redefine capability contract schema, semver rules, or immutability rules — those are inherited as-is from `traverse-framework/traverse`'s specs `005-capability-registry`, `037-semver-range-resolution`, and `043-module-dependency-management`, which moved with the `traverse-registry` crate. Where this spec is silent on a rule those specs already establish, the inherited rule applies.

## User Scenarios & Testing

### User Story 1 - Publish a Capability Through a Reviewed PR (Priority: P1)

As a capability author, I want to submit a new capability contract + artifact reference as a pull request against this repo, so that it becomes part of the governed public registry only after passing automated checks and explicit human review.

**Why this priority**: Publication is the registry's core operation — nothing else in this repo matters if a capability can't safely become part of the registry.

**Independent Test**: Open a PR adding a new file at `capabilities/traverse/example-capability/1.0.0/contract.json` with a valid contract and artifact reference. Verify CI runs deterministic validation, a human approves, and merging produces a new versioned index artifact release.

**Acceptance Scenarios**:

1. **Given** a valid capability contract and artifact reference at the correct path, **When** the PR is opened, **Then** CI validates schema, semver-bump-vs-diff, digest integrity, namespace collision, and dependency resolvability, and reports pass/fail on the PR.
2. **Given** CI passes, **When** a human reviewer approves and merges, **Then** the merge triggers a CI job that builds a new versioned aggregated index artifact and publishes it as a GitHub Release.
3. **Given** CI passes but no human has approved, **When** someone attempts to merge, **Then** the merge is blocked — automated validation alone is never sufficient for publication (inherited from the org constitution, Principle II).

---

### User Story 2 - Detect Likely Duplicate or Low-Quality Capabilities Before Merge (Priority: P1)

As a reviewer, I want an automated pass to flag capabilities that look like semantic duplicates of existing entries, or that look like a CRUD wrapper/utility function disguised as a capability, so that the registry doesn't accumulate junk or redundant entries.

**Why this priority**: Avoiding duplication and maintaining capability quality was the top explicit concern motivating this repo's existence.

**Independent Test**: Open a PR for a capability whose described effect closely overlaps an existing published capability under a different name/tags. Verify the AI-advisory pass posts a PR comment flagging the likely overlap, without blocking merge by itself.

**Acceptance Scenarios**:

1. **Given** a new capability whose description/contract semantically overlaps an already-published capability, **When** CI runs, **Then** an advisory comment is posted identifying the likely duplicate, but merge is not automatically blocked by this signal alone.
2. **Given** a new capability that represents a CRUD wrapper or utility function rather than a meaningful business action, **When** CI runs, **Then** an advisory comment flags the boundary-quality concern for the human reviewer.
3. **Given** the deterministic checks (schema, semver, digest, namespace, dependency resolvability) all pass but the AI-advisory pass raises a flag, **When** a human reviewer disagrees with the flag, **Then** they may still approve and merge — the AI pass never overrides human judgment.

---

### User Story 3 - Sync the Registry Into Local Workspace State (Priority: P1)

As a Traverse operator, I want `traverse-cli registry sync` to fetch the latest published index into local durable state, so that the runtime can resolve capabilities without any live network dependency at execution time.

**Why this priority**: This is what makes the registry actually usable by the runtime — without a sync path, publishing capabilities here has no consumer.

**Independent Test**: Publish a capability (User Story 1), then run `traverse-cli registry sync` from a separate Traverse workspace and verify the capability is resolvable locally afterward, with no further network calls required.

**Acceptance Scenarios**:

1. **Given** a published index release exists, **When** `traverse-cli registry sync` runs, **Then** the CLI fetches the latest release artifact and writes it to local durable workspace state.
2. **Given** local state has been synced, **When** the runtime resolves a capability by identity/version range, **Then** resolution reads only local state — no live call to this repo or any hosted service occurs.
3. **Given** sync has never been run, **When** the runtime attempts to resolve a capability, **Then** resolution fails predictably with an actionable error indicating a sync is required (inherited reliability expectation from the org constitution).

---

### User Story 4 - Deprecate a Published Version Without Breaking Immutability (Priority: P2)

As a registry maintainer, I want to mark a bad or superseded capability version as deprecated ("yanked") so that new range-based resolutions skip it, without deleting or editing the immutable published record.

**Why this priority**: Immutability without a deprecation path means bad publishes accumulate forever with no recourse — a real operational need, but less urgent than the publish/sync path itself.

**Independent Test**: Publish `traverse.example/1.2.0`, then yank it. Verify a consumer resolving `^1.0.0` no longer receives `1.2.0`, while a consumer pinned exactly to `1.2.0` still resolves it successfully.

**Acceptance Scenarios**:

1. **Given** a published version, **When** a maintainer submits a yank PR, **Then** the original contract file is never edited — a separate deprecation-metadata record is added instead.
2. **Given** a yanked version, **When** range-based resolution runs (e.g. `^1.2.0`), **Then** the yanked version is excluded from candidates.
3. **Given** a yanked version, **When** a consumer resolves it by exact pin, **Then** resolution still succeeds — yanking never breaks an existing exact-pin dependency.

---

### Edge Cases

- What happens when two PRs concurrently attempt to publish the same `<namespace>/<id>/<version>` path? The path-encoded layout makes this a git merge conflict, not a silent overwrite — the second PR cannot merge without resolving the conflict, which structurally prevents a race.
- How does the system handle a capability contract that declares a dependency on a capability/version that doesn't exist in this registry yet? Registration-time dependency resolution (inherited from spec `043-module-dependency-management`) fails with `dependency_unsatisfiable` before merge.
- What happens if the AI-advisory pass fails to run (e.g. an API/service outage)? The deterministic checks still gate merge independently; the AI pass failing to run should be visible on the PR but must not silently block merge on its own, since it's advisory only.

## Requirements

### Functional Requirements

- **FR-001**: The repo MUST lay out capability records at `capabilities/<namespace>/<id>/<version>/contract.json`, with artifact references stored alongside.
- **FR-002**: A given `<namespace>/<id>/<version>` path MUST be immutable once merged to `main` — no PR may modify an existing published contract file.
- **FR-003**: CI MUST run deterministic validation on every PR: schema validation, semver-bump-vs-actual-diff-class validation, artifact digest integrity, namespace collision detection, and dependency resolvability.
- **FR-004**: CI MUST run an advisory AI review pass that flags likely semantic duplicates and capability-boundary quality concerns as PR comments, without blocking merge on its own.
- **FR-005**: Merge to `main` MUST require explicit human approval; no combination of automated checks alone may merge a PR (inherited from the org constitution, Principle II).
- **FR-006**: Merging to `main` MUST trigger a CI job that builds a versioned aggregated index artifact and publishes it as a GitHub Release.
- **FR-007**: WASM binary artifacts MUST NOT be committed directly into this git repository; they MUST be stored as GitHub Release assets and referenced from the contract by digest + URL.
- **FR-008**: The capability record schema MUST reserve `owner` and `namespace` fields, defaulted to a core identity and an unscoped namespace respectively, to avoid a breaking migration when third-party publishing is added later.
- **FR-009**: A capability version MUST be deprecable via a separate, additive deprecation-metadata record (yank) that excludes it from range-based resolution while preserving exact-pin resolvability — the original contract file is never edited or removed.
- **FR-010**: This repo MUST NOT implement its own copy of the constitution, quality standards, or spec-alignment script — it vendors a pinned version from `traverse-framework/.github` (see `.specify/memory/constitution.md`).

### Key Entities

- **Capability Record**: a `contract.json` file at `capabilities/<namespace>/<id>/<version>/`, conforming to the schema inherited from `traverse/specs/002-capability-contracts`, plus this repo's reserved `owner`/`namespace` fields.
- **Artifact Reference**: metadata (digest + GitHub Release URL) pointing to the actual WASM binary or source artifact for a capability version.
- **Index Artifact**: the versioned, aggregated manifest built by CI on every merge to `main`, published as a GitHub Release, and fetched by `traverse-cli registry sync`.
- **Deprecation Record**: an additive metadata file marking a specific `<namespace>/<id>/<version>` as yanked, without modifying the original contract file.

## Success Criteria

### Measurable Outcomes

- **SC-001**: A capability author can go from a valid local contract to a merged, publicly resolvable registry entry using only `traverse-cli capability publish` plus one human PR approval — no manual git steps.
- **SC-002**: 100% of merges to `main` are traceable to a passing deterministic-check run and an explicit human approval, with zero exceptions.
- **SC-003**: A `traverse-cli registry sync` followed by a capability resolution completes with zero live network calls to this repo or any hosted service.
- **SC-004**: The registry never contains two capability records at the same `<namespace>/<id>/<version>` path with different content — enforced structurally, not just by CI logic.
- **SC-005**: Yanking a version never breaks a consumer already pinned to that exact version.

## Assumptions

- The only accepted publisher in this spec's scope is the core Traverse team, via PR review — third-party/dev-publisher onboarding (namespace claiming, identity verification, authenticated publish API) is explicitly out of scope for this spec and deferred to a future spec slice.
- A hosted API layer (serverless/edge, pay-per-use) is anticipated but not designed or implemented under this spec — the schema and resolve/fetch interface are kept storage-agnostic so that layer can be added later without a breaking migration.
- The AI-advisory review pass's specific model/tooling/prompt design is not specified here and is deferred to its own implementation spec slice.
- GitHub Actions and GitHub Releases are assumed as the CI and artifact-hosting mechanism for this phase (both free for a public repo), consistent with keeping running costs at zero until real usage justifies a hosted layer.
