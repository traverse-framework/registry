# Feature Specification: Publication Lifecycle Measurement

**Feature Branch**: `022-publication-lifecycle-measurement`
**Created**: 2026-09-06
**Status**: Approved (2026-09-06, v1.0.0)
**Input**: Decided via `/brainstorm` with the repo owner, resolving `registry#354`
("Measure governed capability publication friction and lifecycle outcomes").
Full reasoning, all eleven questions with options and rationale:
`docs/decision-log.md` entry 83. This spec is the "registry telemetry/
publication-policy spec amendment" the owner's `#354` decision-inventory
comment required before implementation; that same decision-log entry is the
ADR it also asked for (this repo keeps ADR-class rationale in the decision
log — `docs/decision-log.md` entry 75 — it has no separate `docs/adr/`).

## Purpose

The value and cost of this registry's publication governance — contract
immutability, the deterministic CI gate, persona authoring, the advisory
review pass, human-merge-only — are not measured. There is no evidence base
for deciding when that governance is justified for a given capability and
where the workflow is heavier than it needs to be.

This spec defines a **privacy-preserving, opt-in, aggregate** measurement of
the publication lifecycle for capability PRs against this repo, plus the
periodic report derived from it and the publication-path guidance that report
is meant to inform. It deliberately collects no capability source, no
payloads, no secrets, and no developer-identifying data — the measurable
lifecycle is derived from this repo's own public GitHub + git history, and the
only author-supplied field is a single complexity-classification override.

## Scope

In scope:

- A per-PR **measurement record** JSON schema (`metrics/publication-lifecycle/schema.json`)
  capturing only: the PR number, a `simple`/`complex` complexity bucket and the
  deterministic rubric inputs behind it, the merge/close outcome, elapsed
  duration in whole hours, review-round count, and counts of CI validation
  failures by stable category. No capability id, namespace, owner, author, or
  wall-clock timestamp.
- A deterministic **complexity rubric** (`simple` vs `complex`) computed from
  non-sensitive contract + PR-diff metadata, with an author override that is
  itself recorded (with a bounded reason) as a data point.
- `scripts/ci/measurement_validation.py` — a deterministic gate that validates
  every `metrics/publication-lifecycle/*.json` record for schema conformance and
  redaction (rejecting unknown fields, identity-bearing content, and
  finer-than-permitted timestamps), recomputes the rubric result to confirm the
  record did not misreport it, and (diff-based) forbids modifying a record once
  merged. Wired into the existing required `capability-validation` CI job and
  into `scripts/ci/pre_pr_check.sh`.
- `scripts/metrics/publication_lifecycle_export.py` — an **opt-in** local helper
  that builds a candidate record for a merged PR from `gh` + local git, applies
  the rubric, accepts an override, and writes the record file for the author to
  review and contribute via a normal PR.
- `metrics/publication-lifecycle/` — the record directory, its `README.md`, and
  the schema.
- `docs/publication-lifecycle-report.md` — a hand-written report document with
  its methodology section, published with no report edition until the record
  threshold is met.
- `CONTRIBUTING.md` guidance on the **lightweight/private path**: which
  capabilities may stay app-private (and where), and that going public always
  requires the full public gates with no shortcut and no in-place promotion.

Out of scope:

- Any instrumentation of `traverse-cli` or `crates/traverse-registry/`, and any
  "draft start" / pre-PR authoring timing. The record begins at PR open. A
  future author-reported or `traverse-cli`-sourced draft-start field would be a
  separate spec amendment (decision-log entry 83, Q4).
- Building an actual reduced-gate private publication track. `#354` decides the
  private path is **documentation only** here; a real lighter-weight mechanism,
  if demand appears, is its own future issue and spec (decision-log entry 83,
  Q3).
- Central or automatic collection of any kind. Collection is opt-in and defaults
  off: a record exists only when an author runs the helper and opens a PR
  adding it.
- Retroactively producing records for historical PRs as a batch. Nothing
  prevents an author contributing a record for an older PR of their own, but no
  automated backfill is defined or run.
- Any consumption of a record or the report by a CI gate, the resolver, the
  catalog, or an approval decision. The report is prose for humans.

## Requirements

### Functional Requirements

#### Measurement record

- **FR-001**: A measurement record is a JSON object stored at
  `metrics/publication-lifecycle/pr-<n>.json`, where `<n>` is the PR number and
  matches the record's `pr_number`. Records conform to
  `metrics/publication-lifecycle/schema.json`. The permitted top-level fields
  are exactly: `record_schema_version`, `pr_number`, `period`, `outcome`,
  `hours_open_to_conclusion`, `review_rounds`, `complexity`, `rubric`,
  `ci_failure_counts`. Any other top-level field MUST fail validation.
- **FR-002**: A record MUST NOT contain — at any depth, as a key or a string
  value — a capability id, namespace, `owner`, `team`, `author`, `publisher`,
  `login`, `email`, or `user`; a string matching an email address; a string
  containing an `@`-prefixed handle; or a URL. `measurement_validation.py` MUST
  reject any record that does.
- **FR-003**: `period` MUST be a calendar month, format `^\d{4}-\d{2}$` — the
  month the PR reached its outcome. Any finer timestamp precision (a day, a
  time, an ISO-8601 instant) anywhere in the record MUST fail validation. This
  is the concrete "no timestamps finer than permitted" redaction rule.
- **FR-004**: `outcome` MUST be one of `merged` or `closed_unmerged`.
  `hours_open_to_conclusion` MUST be a non-negative integer (whole hours,
  PR-open to merge or close). `review_rounds` MUST be a non-negative integer.
- **FR-005**: `complexity` MUST be an object with exactly `computed`, `final`,
  `override`, and `override_reason`. `computed` and `final` are each `simple`
  or `complex`. `override` is a boolean. When `override` is `false`, `final`
  MUST equal `computed` and `override_reason` MUST be `null`. When `override`
  is `true`, `final` MUST differ from `computed` and `override_reason` MUST be a
  non-empty string of at most 200 characters, subject to FR-002's redaction
  rules.
- **FR-006**: `rubric` MUST be an object with exactly `effectful` (bool),
  `dependency_count` (non-negative int), `changed_specs` (bool),
  `added_persona` (bool), `schema_field_count` (non-negative int), and
  `schema_field_threshold` (positive int). `measurement_validation.py` MUST
  recompute the rubric verdict from these six values (per FR-009) and fail the
  record if it does not equal `complexity.computed` — a record cannot misreport
  its own rubric result.
- **FR-007**: `ci_failure_counts` MUST be an object whose keys are drawn only
  from the stable category set in FR-010 and whose values are non-negative
  integers. Absent categories are treated as zero; unknown keys MUST fail
  validation.

#### Complexity rubric

- **FR-008**: The rubric classifies a publication as `simple` when **all** of
  the following hold, and `complex` otherwise:
  1. `effectful` is `false` — the added contract is pure/deterministic: every
     `side_effects[].kind` is `memory_only`, `execution.constraints`
     `host_api_access`/`filesystem_access` are `none`, `network_access` is
     `forbidden`, and `connector_requirements`, `emits`, and `consumes` are all
     empty;
  2. `dependency_count` is `0` — the contract's `dependencies` array is empty;
  3. `changed_specs` is `false` — the PR changes no file under `specs/`;
  4. `added_persona` is `false` — the PR adds no new `personas/**/persona.json`;
  5. `schema_field_count <= schema_field_threshold`.
- **FR-009**: `schema_field_count` is the total number of `properties` keys,
  counted recursively, across the added contract's `inputs.schema` and
  `outputs.schema`. `schema_field_threshold` defaults to **20**. The threshold
  is a tuning knob: a future change MAY set a different default in the schema
  and the helper without a spec amendment, provided the value stays recorded
  per-record so past records remain interpretable.
- **FR-010**: The stable CI-failure category set is:
  `schema`, `semver`, `digest`, `namespace_collision`, `dependency_resolvability`,
  `immutability`, `persona_ref`, `use_cases_format`, `use_cases_coverage`,
  `action_enum_coverage`, `artifact_reference`, `test_coverage`, `signature`,
  `ecca_inventory`, `event_product`, `spec_alignment`, `other`. A failure that
  does not map to a specific category is counted under `other`.

#### Validation gate

- **FR-011**: `scripts/ci/measurement_validation.py` run with no arguments MUST
  validate the shape (FR-001 through FR-010) of every record under
  `metrics/publication-lifecycle/`. Run with `<base_sha> <head_sha>` it MUST
  additionally fail if any existing `metrics/publication-lifecycle/*.json` is
  modified or deleted in that range (records are immutable once merged; a
  correction is a new record for a new PR, never an edit — mirroring
  `capability_validation.py`'s `check_immutability`). It MUST print
  `{"status": "...", "failures": [...]}` and exit non-zero on any failure, the
  same contract as `capability_validation.py`.
- **FR-012**: The `capability-validation` GitHub Actions job MUST run
  `measurement_validation.py` (diff-based on PRs, whole-tree on push) and its
  unit tests. This gate rides the existing required `capability-validation`
  check rather than a new job — making a new job a required check is an
  owner-only branch-protection change this repo's automation does not make
  itself (precedent: `docs/decision-log.md` entries 28, 39). `scripts/ci/pre_pr_check.sh`
  MUST run `measurement_validation.py` too, so a contributor reproduces the gate
  locally.
- **FR-013**: The validator MUST NOT require any network access, `gh`, or cargo.
  It reads only the record files and `git diff --name-status`.

#### Export helper

- **FR-014**: `scripts/metrics/publication_lifecycle_export.py --pr <n>` MUST
  build a candidate record for a merged or closed PR: `period`, `outcome`,
  `hours_open_to_conclusion`, and `review_rounds` from `gh` PR metadata;
  `rubric` and `complexity.computed` from the PR's added `contract.json` and
  changed-file list at its merge commit. `--set-complexity simple|complex`
  with `--reason "<text>"` records an override. It writes
  `metrics/publication-lifecycle/pr-<n>.json` and prints the path. It never
  contributes anything itself — the author reviews the file and opens a PR.
- **FR-015**: The helper is opt-in by construction: it is not invoked by CI, by
  `traverse-cli`, or by any merge hook. Nothing in this repo produces a record
  without an author running it and opening a PR.

#### Report and guidance

- **FR-016**: `docs/publication-lifecycle-report.md` MUST exist with a
  methodology section documenting: that records are opt-in and self-selected
  (and the selection bias that implies), the sample size, the data-retention
  policy (records are immutable and retained indefinitely; each report edition
  is superseded in place with prior editions kept in git history), and how
  uncertainty is expressed. Until the threshold in FR-017 is met it MUST state
  plainly that no edition has been published and show `n = 0`.
- **FR-017**: A report edition MUST separate the `simple` and `complex` buckets
  and, per bucket, report the record count and the median and 90th-percentile
  `hours_open_to_conclusion`, plus the frequency of each `ci_failure_counts`
  category. The first edition is published once at least **20** records exist;
  thereafter a new edition is published for each additional 20. The threshold
  is a tuning knob recorded in the report, changeable without a spec amendment.
- **FR-018**: The report and the records MUST NOT be consumed by any automated
  mechanism — no CI gate, no resolver behavior, no catalog field, no approval
  decision reads them. The report carries no per-capability or per-author
  breakdown; the schema's FR-002 prohibitions make one impossible from the
  records alone. This is the structural form of `#354`'s "no hidden ranking or
  automatic approval mechanism" requirement.
- **FR-019**: `CONTRIBUTING.md` MUST document the lightweight/private path:
  app-private or workspace-scoped capabilities are not published to this
  registry and stay in the consumer's own workspace; publishing to this
  registry always requires the full public validation, review, and coverage
  gates with no reduced path; and promotion of a previously-private capability
  is a fresh public publication, never an in-place upgrade of a private record.

## Success Criteria

- **SC-001**: A record with a field outside the FR-001 set, or containing an
  email / `@handle` / URL / capability-identifying key at any depth, is
  rejected by `measurement_validation.py` with a message naming the offending
  path.
- **SC-002**: A record whose `rubric` values imply `simple` but whose
  `complexity.computed` says `complex` (or vice versa) is rejected.
- **SC-003**: A record with `override: true` and `final == computed`, or
  `override: true` with an empty/oversized/identity-bearing `override_reason`,
  or `override: false` with a non-null `override_reason`, is rejected.
- **SC-004**: A record with `period` at day precision (`2026-09-06`) or
  carrying any ISO-8601 instant anywhere is rejected.
- **SC-005**: A PR that modifies or deletes an already-merged
  `metrics/publication-lifecycle/*.json` fails `measurement_validation.py` in
  its diff-based mode.
- **SC-006**: A well-formed record passes both the whole-tree and diff-based
  runs, and the `capability-validation` job and `pre_pr_check.sh` both execute
  the gate.
- **SC-007**: `docs/publication-lifecycle-report.md` exists, states `n = 0` and
  "no edition published", and documents sample size, selection bias, retention,
  and uncertainty.
- **SC-008**: `CONTRIBUTING.md` states the private-path boundary and the
  no-shortcut-to-public / no-in-place-promotion rule (FR-019).

## Governing Relationship

Additive to `001-registry-foundation`: this spec does not amend `001`'s text,
it adds a new opt-in measurement surface and a new required-gate step over
newly-governed paths (`metrics/`, `scripts/metrics/`,
`scripts/ci/measurement_validation.py`). It does not change any requirement of
`002-capability-validation`, `015-runtime-usage-telemetry-resolve-hook` (a
distinct, crate-level resolve-hook telemetry — not publication lifecycle),
`017-persona-registry`, or `018-capability-test-coverage`. It changes no
`crates/traverse-registry/` behavior; the touch to that crate's bundled
`governance/approved-specs.json` copy is only the mandatory in-sync mirror of
`specs/governance/approved-specs.json` (`013-inherited-registry-governance`).
