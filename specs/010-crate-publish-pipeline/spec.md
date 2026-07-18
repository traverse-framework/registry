# Feature Specification: Crate Publish Pipeline

**Feature Branch**: `010-crate-publish-pipeline`
**Created**: 2026-07-18
**Status**: Approved
**Input**: Prerequisite work for `traverse` issue #627 / this repo's #9 (extract the `traverse-registry` Rust crate from `traverse-framework/traverse` into this repo, per `traverse` spec `051-registry-extraction`). Decided in a `/brainstorm` session with the repo owner, 2026-07-18. Full reasoning: `docs/decision-log.md` entry 29.

## Purpose

This repo has no executable code or Cargo structure today — it is a git-based capability/spec registry only (decision log entry 3). Before the real `traverse-registry` crate content can move here (#9), this repo needs:

- a Cargo package that can build and publish to crates.io
- a CI pipeline that publishes it, mirroring `traverse`'s own tag-triggered publish job rather than inventing a new mechanism (decision log entry 7 already established this org's convention: reuse `traverse`'s existing pipeline shapes instead of designing new ones)
- the `traverse-registry` name reserved on crates.io immediately, as a placeholder, to close off the exact risk that already cost this org a rename (`traverse-cli` → `traverse-cli-rs`, `traverse` PR #743, days before this spec)

This spec governs only the scaffold and pipeline. It does not move any crate content — that remains #9's scope, sequenced to land only after this spec's pipeline is proven working (a real placeholder publish succeeds).

## Design Decisions

### Placeholder Crate, Published Immediately Under the Final Name

A minimal `traverse-registry` package (a single crate-level doc comment stating it is a name-reservation placeholder pending the real extraction, no real logic) is published to crates.io as `0.0.1` as soon as this spec's pipeline works. This reserves the name now rather than at content-move time, when a collision would be far more costly to discover.

The real crate content (#9) later publishes as `0.8.0` directly over this placeholder — an ordinary increasing-semver publish, not a special-cased transition. crates.io places no constraint on the size of a version jump.

### Publish Trigger: Tag Push Only, Mirroring `traverse`

`traverse`'s own `publish` CI job runs only `on: push` to a `refs/tags/v*` ref, gated behind its full deterministic check suite (`.github/workflows/ci.yml`). This repo adopts the identical trigger shape: a version tag push on `main`, not every merge, and not a manual `workflow_dispatch`. No new mechanism is introduced.

### Governing Spec Scope

This spec's `governs` covers the new Cargo/CI surface only: `Cargo.toml`, `crates/traverse-registry/` (the eventual scaffold + later real content), `.github/workflows/publish-crate.yml`, and `scripts/ci/publish_crate.sh`. It is deliberately separate from the specs being re-adopted for #9 (005/007/011/034/035/036/037/039/041/043), which govern registry/capability *semantics* inherited from `traverse` — this spec governs build/publish *tooling*, a different concern that those specs were never written to cover.

### Secret Dependency (Owner Action, Not Agent-Executed)

The pipeline requires a `CARGO_REGISTRY_TOKEN` secret. `traverse-framework` already has this as an org-level secret (used by `traverse`'s own publish job), but it is currently scoped to specific repos that does not yet include this one. Per this repo's existing precedent (decision log entry 28: branch-protection/required-status-check changes are a security-settings change outside what an agent may do regardless of approval), granting this repo access to that org secret is the same class of action — the repo owner does it directly in GitHub's org settings UI, not an agent. This spec's pipeline is correct and complete once merged; it simply fails safely (a clear, actionable CI error) on the first tag-triggered run until the owner grants access, exactly like decision log entry 19's `ai-advisory-review` job did for `ANTHROPIC_API_KEY`.

## User Scenarios & Testing

### User Story 1 - Reserve the Crate Name Before Real Content Exists (Priority: P1)

As the repo owner, I want `traverse-registry` claimed on crates.io as soon as this repo can publish anything at all, so the real crate extraction (#9) never risks arriving at an already-taken name.

**Why this priority**: this is the entire reason this spec exists ahead of #9 rather than being folded into it — the failure mode (name collision discovered at real-content-publish time) already happened once in this org, days ago, to a sibling crate.

**Independent Test**: `cargo search traverse-registry` (or the crates.io API) returns a published `0.0.1` placeholder before #9's PR opens.

**Acceptance Scenarios**:

1. **Given** this spec's scaffold and CI merged to `main`, **When** a `v0.0.1` tag is pushed, **Then** the publish job builds and runs `cargo publish` for the placeholder package.
2. **Given** the `CARGO_REGISTRY_TOKEN` secret is not yet granted to this repo, **When** the publish job runs, **Then** it fails with a clear, actionable error (missing credential) rather than a silent no-op or a misleading unrelated failure.
3. **Given** the placeholder is published, **When** `traverse`'s `Cargo.toml` is later inspected for the collision this spec exists to prevent, **Then** `traverse-registry` on crates.io is already owned by this org.

---

### User Story 2 - Prove the Pipeline Independently of the Content Move (Priority: P2)

As the repo owner, I want the publish mechanics validated on trivial content, so that when the real `traverse-registry` source lands (#9), a broken publish pipeline and a broken content move are never entangled in the same failure.

**Independent Test**: The placeholder publish succeeds end-to-end (tag push → CI build → `cargo publish` → package visible on crates.io) with zero content beyond the placeholder doc comment.

**Acceptance Scenarios**:

1. **Given** the placeholder package, **When** CI runs on a tag push, **Then** it passes this repo's existing deterministic gates (`spec-alignment`, and any build/test steps this spec adds) before attempting to publish.
2. **Given** a successful placeholder publish, **When** #9's real content later replaces the placeholder source and a new tag is pushed, **Then** the same, unmodified pipeline publishes it — no pipeline changes are needed at content-move time.

### Edge Cases

- **Tag pushed before the secret is granted**: publish job fails cleanly at the `cargo publish` step with the credential error surfaced in CI logs; the scaffold and build steps that ran before it are unaffected and require no changes once the secret is later granted — re-pushing the same tag (or a corrected one) succeeds without any pipeline edit.
- **Someone else claims the name in the gap between this spec's approval and the placeholder actually publishing**: out of scope to defend against (no known reservation mechanism on crates.io exists short of publishing); mitigated only by prioritizing this spec ahead of #9 rather than deferring the reservation.

## Requirements

### Functional Requirements

- **FR-001**: This repo MUST contain a Cargo package named `traverse-registry`, initially a minimal placeholder crate (a crate-level doc comment only, no functional code), buildable with `cargo build`.
- **FR-002**: This repo MUST have a CI publish workflow that runs `cargo publish` for the `traverse-registry` package, triggered only on push of a `refs/tags/v*` ref to `main` — mirroring `traverse`'s existing `.github/workflows/ci.yml` `publish` job trigger shape.
- **FR-003**: The publish workflow MUST run this repo's existing deterministic CI gates before attempting `cargo publish`, so a broken build or gate failure blocks publish rather than racing it.
- **FR-004**: The publish workflow MUST read the publish credential from a `CARGO_REGISTRY_TOKEN` secret and MUST fail with a clear, actionable error if that secret is absent — never silently skip the publish step.
- **FR-005**: The placeholder package MUST be published as `0.0.1` under the exact name `traverse-registry`, with no functional API surface, before #9's real content-move PR is opened.
- **FR-006**: This spec's pipeline MUST require no changes when #9's real crate content later replaces the placeholder source — the same workflow, trigger, and gate sequence publish the real `0.8.0` release.

## Success Criteria

- **SC-001**: `traverse-registry` is a published, owned package on crates.io before #9's content-move PR exists.
- **SC-002**: A tag push is the only way `cargo publish` runs for this package — no manual `cargo publish` from a local machine is part of the supported flow.
- **SC-003**: #9's eventual real-content PR requires zero edits to `.github/workflows/publish-crate.yml` or `scripts/ci/publish_crate.sh` to succeed.

## Assumptions

- `traverse-framework`'s `CARGO_REGISTRY_TOKEN` org secret is the correct credential to reuse (same publishing identity `traverse` already uses for its own crates) rather than provisioning a separate token — consistent with this being one org publishing under one crates.io ownership.
- crates.io permits an arbitrarily large version jump between `0.0.1` and `0.8.0` for the same package name with no special process — standard crates.io behavior, not an assumption specific to this org.
