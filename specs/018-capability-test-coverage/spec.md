# Feature Specification: Capability Test Coverage

**Feature Branch**: `018-capability-test-coverage`
**Created**: 2026-08-21
**Status**: Approved
**Input**: Decided via direct owner scoping conversation, 2026-08-21. Full reasoning: `docs/decision-log.md` entry 64.

## Purpose

`CLAUDE.md`'s own Code Style section states "100% coverage for core logic" as this repo's inherited bar, and `gather_catalog_data.py` has measured `test_coverage` via `cargo llvm-cov` for the catalog display since issue #255/#264 — but nothing in this repo's CI has ever actually *gated* a publish on that bar; measurement has been advisory-only. Auditing this repo's real coverage numbers while scoping this spec found the gap was far wider than assumed:

- 18 of 46 published capabilities (`owner.team: "callweave"`) have no `capability-src/` source in this repo at all — not a coverage shortfall but an absence of anything to measure (tracked: registry#302).
- Of the 28 capabilities that do have source, only 11 met even a relaxed bar before this spec's own investigation triggered backfill work. Two (`core-calculate-deadline-pressure`, `core-decide-escalation`) turned out to have literal unresolved git conflict markers committed on `main` since 2026-08-20 — not a coverage gap, active corruption, fixed in registry#299. Two more (`platform-decide-state-transition`, `core-record-nudge-event`) were severely under-tested (21% and 43% line coverage) despite issue #273 having closed under the title "bring test coverage to this repo's 100% bar" — that closure covered a different pair of crates. Both backfilled in registry#300. The remaining 15 are tracked, not blocking, in registry#301.
- No crate in this repo, even the best-tested ones, has ever measured literal 100% on lines and regions — real test suites here consistently land at 95-99% due to genuinely unreachable defensive branches (e.g. an `unwrap_or` fallback after upstream validation already guarantees the `None` case cannot occur). Mandating literal 100% across all three `cargo llvm-cov` metrics would set a bar nothing in this repo has ever cleared.

This spec makes coverage a required, CI-enforced gate for every future capability publish — internal or external — rather than an advisory catalog display, while being honest about what "100%" operationalizes to given the above.

## Scope

In scope:

- A required, diff-based CI check (`scripts/ci/capability_validation.py`) that fails a PR adding a new `capabilities/**/contract.json` unless a corresponding `capability-src/<crate>/` exists and measures, via `cargo llvm-cov --summary-only --json`, `functions.percent == 100.0` and `lines.percent >= 95.0` and `regions.percent >= 95.0`.
- A canonical, deterministic crate-naming rule for capabilities published after this spec takes effect: `capability-src/<capability_id with every "." replaced by "-">/`. This removes the need for any hand-maintained id-to-crate mapping (the "no magic" policy `gather_catalog_data.py`'s `CURRENT_CRATE_FOR_ID` and `scripts/ci/verify_use_cases.py` already use for pre-existing, inconsistently-named crates) for anything new going forward.
- The same bar applies to external contributors (Callweave-style) as to internal ones: mandatory `capability-src/` source in the same PR, not a self-reported coverage attestation. An unverifiable external claim is exactly the failure mode decision-log entry 61 already fixed once (stopped trusting `traverse-cli`'s self-reported "passed" instead of independently checking) — this spec does not reintroduce it.
- CI tooling: installing `cargo-llvm-cov` + the `llvm-tools-preview` component in the `capability-validation` job so the new check can actually run.
- `CONTRIBUTING.md` documentation of the requirement, including for contributors outside this repo's usual working group.

Out of scope:

- Retroactively gating any already-published capability — diff-based, identical treatment to every other content check `capability_validation.py` already runs (`check_new_scenarios_are_user_stories`, `check_new_use_cases_have_persona_ref`, `check_new_contracts_have_artifact_reference`, etc.), all of which explicitly never re-judge historical immutable content because doing so would fail permanently on content that can never be edited to comply.
- The 18 Callweave capabilities with no source at all (tracked: registry#302, requesting source from the owning team — not this repo's call to backfill unilaterally, see "Why not backfilled" below).
- The 15 existing capability-src crates below the new bar that aren't corrupted or catastrophically under-tested (tracked: registry#301).
- Literal 100% on lines and regions (only functions is held to literal 100%; lines/regions use a ≥95% floor, per the Purpose section's measured evidence that no crate in this repo has ever cleared a literal-100%-on-all-three bar).
- Any change to `gather_catalog_data.py`'s existing advisory `CURRENT_CRATE_FOR_ID` mechanism for pre-existing crates — this spec's naming rule (id with dots replaced by dashes) applies only to capabilities published after this spec takes effect; existing inconsistently-named crates keep their existing explicit mapping entries unchanged.

## Requirements

### Functional Requirements

- **FR-001**: A newly-ADDED `capabilities/<namespace>/<id>/<version>/contract.json` (diff-based: only files with git status `A` in the PR's diff against `origin/main`, mirroring every other diff-based check in `capability_validation.py`) MUST have a corresponding `capability-src/<id-with-dots-replaced-by-dashes>/Cargo.toml`. A missing crate directory MUST fail CI with a message identifying the expected path.
- **FR-002**: The crate at that path MUST build and its test suite MUST pass under `cargo llvm-cov --summary-only --json --manifest-path <path>/Cargo.toml`. A build failure or test failure MUST fail CI with the raw tool error, not a soft skip.
- **FR-003**: The measured `data[0].totals` MUST satisfy `functions.percent == 100.0`, `lines.percent >= 95.0`, and `regions.percent >= 95.0`. Any shortfall MUST fail CI with the exact measured percentages so an author knows precisely what to add.
- **FR-004**: This check MUST NOT run against any `contract.json` that predates this spec (i.e. any file not freshly `A`-status in the current PR's diff) — identical diff-based scoping to `check_new_contract_artifact_reference` and its documented rationale.
- **FR-005**: External contributors (any `owner.team` other than this repo's own working group) are held to the identical FR-001 through FR-003 bar — no attestation-only or self-reported-coverage path is accepted in place of real, independently-measurable source in this repo.
- **FR-006**: The `capability-validation` CI job MUST install `cargo-llvm-cov` and the `llvm-tools-preview` rustup component (mirroring the existing `build-catalog` job's already-working recipe) so FR-002/FR-003 can actually execute.
- **FR-007**: `CONTRIBUTING.md` MUST document this requirement plainly enough for a contributor outside this repo's usual working group to follow without needing to read this spec or `capability_validation.py` source: what `capability-src/<crate>/` must contain, the naming rule, the exact `cargo llvm-cov` invocation to check locally before opening a PR, and that source is mandatory regardless of publisher.

## Success Criteria

- **SC-001**: A new capability PR adding a `contract.json` with no corresponding `capability-src/` crate is rejected by CI before merge, with a clear path-identifying message.
- **SC-002**: A new capability PR whose crate builds and tests but falls short of the FR-003 thresholds is rejected by CI with the exact measured percentages.
- **SC-003**: A new capability PR whose crate meets FR-003 passes this check cleanly.
- **SC-004**: No existing, already-published capability (including the 18 Callweave ones and the 15 tracked in registry#301) is newly failed by this check merely by this spec merging.
- **SC-005**: A contributor who has never touched this repo before can, from `CONTRIBUTING.md` alone, determine the exact crate path, naming rule, and local command to verify compliance before opening a PR.

## Governing Relationship

This spec is additive to `001-registry-foundation` — it does not amend that spec's text, it adds a new required gate over content `001` already governs (`capabilities/`) plus a newly-governed path (`capability-src/`, previously ungoverned by any spec's `governs` list). It does not touch `017-persona-registry`, `006-public-scope-and-identity`, or any other existing spec's requirements.
