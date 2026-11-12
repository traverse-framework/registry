# LLM-Assisted Authoring Evaluation

**Status: complete. Bar not met; the low-risk tier stays `experimental` (decision-log entry 139).**

This document is the retrospective evaluation of LLM-assisted capability
authoring that [`specs/023-authoring-assurance`](../specs/023-authoring-assurance/spec.md)
defers to `registry#371`. It was designed in `docs/decision-log.md` entry 138
(`/brainstorm 371`, 2026-11-10).

Everything in the Methodology section was merged **before** any data was
collected, so the bar was fixed before the numbers existed, not chosen after
seeing them. Results are added in a separate, later PR. Changing anything in
the Methodology section after this file first merges requires a new
decision-log entry, and that entry must say whether results had been seen.

---

## Methodology (fixed in advance)

### Question

Is LLM-assisted authoring of low-risk, deterministic capabilities reliable
enough to move spec 023 FR-004's low-risk tier from `experimental` to
`supported`?

### Kind of evaluation

Retrospective. The evaluation mines the registry's existing PR, CI, issue and
deprecation history. It does not run a new controlled authoring experiment;
that is a separate Deferred ticket (see "Out of scope").

Known limits, stated up front:

- **No human baseline.** No published contract declares
  `authoring.method: "human"`, so nothing is compared against human
  authoring. The bar is absolute.
- **Confounded.** One owner directed almost all of this work, the capabilities
  were self-selected, and the CI gates changed over the period measured.
- **The evaluator is an LLM agent judging LLM-authored work.** "Classification"
  below explains how that is handled.

### Cutoff date

**2026-11-10.** Only versions whose publishing PR merged on or before
**2026-10-11**, which is 30 days before the cutoff, are counted. This leaves
each counted version at least 30 days to be used and to fail. Escape evidence
(see "Defect escape") is considered up to the date the results PR collects its
data. That date is recorded in the results.

### Unit of analysis

One **new-artifact version** is one capability version that meets all of
these:

- its `contract.json` declares `authoring.method: "llm-assisted"`;
- it has no `artifact_reuse` marker, neither on the contract nor as a sibling
  `artifact_reuse.json`, so it shipped a new WASM artifact rather than
  republishing metadata over an existing one;
- its publishing PR merged on or before 2026-10-11.

A bug-fix release counts as its own unit, because it was its own authoring
attempt. Metadata-only successors are excluded, because they contain no new
code and would inflate the pass rate.

### Promotion bucket

The bar applies only to versions whose contract declares all of:

- `risk.effect_class: "pure_read"`
- `risk.determinism_class: "deterministic"`
- no `ai.model_backed: true`

Model-backed agents are reported separately and never count toward the bar,
because their failure modes are different (for example, hallucination on
non-speech, #477). Versions without spec 024 `risk` metadata are reported as
"unclassified" and excluded. They are not classified by hand, because risk
classification is forward-only.

### Defect escape

A counted version is an **escape** when, after it merged, it was fixed or
deprecated for either of these reasons:

- **Behaviour:** the guest produced wrong output, or execution failed, on
  input its contract accepts. Examples of the class: #492 (`text.truncate`
  wiped text on a schema-legal `max_length`), #611 (NER entity fragmentation),
  #477 (Whisper hallucination on non-speech).
- **Contract:** consumers could not use the contract as published, for
  example because Traverse's `traverse-contracts` parser rejected it (#656).

Evidence must be at least one of the following, and is linked per version:

- a `deprecated.json` whose `reason` names the defect;
- an issue labeled `bug` that names the version;
- a successor whose `provenance.spec_ref` or `artifact_reuse` justification
  says it fixes the version.

These are **not** escapes: planned evolution through a new minor version
(added features, new fields, schema alignment that was not a consumer-facing
failure), and failures caught by CI before merge.

### Bar

The low-risk tier meets the bar when **all three** hold for the promotion
bucket:

1. **n ≥ 20** counted versions;
2. **escape rate ≤ 10%** (escapes divided by n);
3. **every escape's root cause is now caught by a required CI gate.** Each
   escape names the gate, for example the wasm32 host-limits gate (entry 132)
   or the `traverse-contracts` parse gate (entry 137).

If n is below 20, the bar is **not met**. The thresholds are not loosened to
reach a result.

Meeting the bar does not promote the tier by itself. Promotion still needs a
standalone owner decision recorded in `docs/decision-log.md` and an amendment
to spec 023 FR-004.

### Metrics reported

Defect escapes drive the bar. The results also report these, because they can
be reproduced from GitHub data:

- **First-pass CI rate:** the share of counted versions whose publishing PR
  passed every required check (`capability-validation`, `spec-alignment`) on
  its first push.
- **Gate rejections by failure code:** across all pushes to those PRs, the
  number of failures per `capability_validation.py` code.

Reported as **not measurable retrospectively**:

- **Reviewer rework** and **time to accepted artifact.** With one owner
  merging agent PRs, these measure workflow, not authoring quality.

Model-backed and unclassified versions get the same metrics, reported in their
own rows, so nothing is hidden. They never count toward the bar.

### Classification

The agent collecting the data drafts a per-version evidence table:

| version | bucket | merged | first-pass CI | escape? | evidence | root-cause gate |
|---|---|---|---|---|---|---|

Every row gives its links and a one-line reason for the proposed call. **The
owner confirms every row's escape / no-escape call** in the results PR review
before the totals are computed. Rows the owner changes are marked as changed,
with the reason.

---

## Out of scope

- **A controlled authoring run.** A fixed corpus, a protocol and a human
  baseline, plus contract-derived and property-based tests, are moved to a
  separate Deferred ticket. It triggers when the bar is not met, or when the
  first bounded medium-risk promotion question comes up (#659).
- **Bounded medium-risk and high-risk tiers.** FR-004's stance for these is
  unchanged.

## Results

**Owner sign-off: 2026-11-10.** The owner confirmed every row below as
proposed, including flags A–F. No rows were changed. Totals were computed only
after that sign-off.

### Collection

- **Collected:** 2026-11-10, against `main` at `3d54b92` (the merge of the
  methodology, #660). Escape evidence is considered up to this date.
- **Enumeration:** every `capabilities/*/*/*/contract.json` declaring
  `authoring.method: "llm-assisted"` (134 versions). 62 have no
  `artifact_reuse` marker; 61 of those merged on or before 2026-10-11.
  The merge date is the date of the squash commit on `main` that added the
  `contract.json`.
- **Buckets** (from each contract's own `risk` and `ai` fields): promotion
  25, model-backed 8, other risk classes 16, unclassified 12.
- **Escape evidence:** every counted version's `deprecated.json`; every issue
  labeled `bug` (6, all states); and the `provenance.spec_ref` and
  `artifact_reuse` justification of every later version of each counted id.
- **First-pass CI:** for each publishing PR, every `ci.yml` run on its head
  branch since the PR was opened, ordered by time, so force-pushed SHAs are
  included. "First push" is the earliest SHA. "yes" means both
  `capability-validation` and `spec-alignment` concluded `success` on it.
- **Gate rejections:** codes are read from the `capability-validation` job
  log of every failed push, counted once per push in which they appear. A PR
  that published several versions (for example #389) is counted once.

### Flags for the owner

Rows marked with a flag are calls the evidence rules do not settle on their
own. The owner accepted every proposed call below.

- **A. `text.truncate@1.0.0`.** The methodology names #492 as an example of a
  behaviour escape, but its rules require the version to have been "fixed or
  deprecated" with evidence of a specified kind. #492 is open, nothing is
  fixed or deprecated, and it is labeled `good first issue`, not `bug`.
  Proposed: **escape**, because the defect is confirmed in review and
  excluding it only because no fix has landed yet would reward an unfixed
  defect. Its root cause (wasm32 `usize` width, invisible to 64-bit host
  tests) has been gated since #509 (#551, merged 2026-09-16), which runs each
  new contract's wasm32 artifact with a `2^32` fixture for unbounded
  length/count integer fields. (Corrected 2026-11-10, decision-log entry 142:
  the results first said no gate caught it.)
- **B. `report.{enrich-insights,summarize,translate-fr}@1.0.0`.** Superseded
  by 1.1.0 so that the chain plans in `browserLocalPlan` (#441). The planner
  composes only linear chains. Its own bug was traverse#1338. Proposed:
  **no escape.** Each contract was usable on its own, and the reshape adapted
  to a planner limitation rather than fixing a defect in the version.
- **C. `commerce.{cart,pricing-config}@1.0.0`.** Deprecated for string-encoded
  JSON fields instead of the structured Wave 2 schema (#407). Proposed:
  **escape (contract).** No gate is named for it.
- **D. `identity.{challenge,principal}-session@1.0.0`.** Deprecated after a
  duplicate publish PR overwrote their release bytes (#405). Proposed:
  **no escape.** This was a release-process failure, not authored content.
- **E. `inference.evidence-normalize@1.0.1`.** Its WASM is byte-identical to
  1.0.0, but it has no `artifact_reuse` marker, so the unit rule counts it.
  Proposed: keep it as counted, as the rule says, with **no escape**. It is
  unclassified and does not affect the bar.
- **F. First-pass CI not determinable.** `text.truncate@1.0.0` (#487): the
  first SHA has a CI run but no required-check results. `core.consent-purpose-check@1.0.0`
  (#595): the PR returns 404 and the merge commit has no associated PR, so
  its CI history cannot be retrieved. Both are reported and left out of the
  first-pass denominator.

### Per-version evidence table

#### promotion (25)

| version | merged | PR | first-pass CI | proposed call | evidence / reason | root-cause gate |
|---|---|---|---|---|---|---|
| `artifact.shareability-classify@1.0.0` | 2026-09-15 | [#518](https://github.com/traverse-framework/registry/pull/518) | yes | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `audio.activity-interval-classify@1.0.0` | 2026-09-18 | [#585](https://github.com/traverse-framework/registry/pull/585) | no | escape | contract: `deprecated.json` names a `traverse-contracts` parse failure ([#656](https://github.com/traverse-framework/registry/issues/656)); lifecycle `experimental` | `traverse-contracts` parse gate (entry 137) |
| `audio.pcm-transform@1.0.0` | 2026-09-15 | [#514](https://github.com/traverse-framework/registry/pull/514) | yes | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `audio.pcm-window@1.0.0` | 2026-09-15 | [#516](https://github.com/traverse-framework/registry/pull/516) | yes | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `audio.privacy-risk-evaluate@1.0.0` | 2026-09-15 | [#519](https://github.com/traverse-framework/registry/pull/519) | yes | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `audio.signal-quality-evaluate@1.0.0` | 2026-09-18 | [#584](https://github.com/traverse-framework/registry/pull/584) | no | escape | contract: `deprecated.json` names a `traverse-contracts` parse failure ([#656](https://github.com/traverse-framework/registry/issues/656)); lifecycle `experimental` | `traverse-contracts` parse gate (entry 137) |
| `audio.wav-pcm16-decode@1.0.0` | 2026-09-15 | [#517](https://github.com/traverse-framework/registry/pull/517) | yes | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `candidate.policy-evaluate@1.0.0` | 2026-09-15 | [#520](https://github.com/traverse-framework/registry/pull/520) | yes | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `core.consent-purpose-check@1.0.0` | 2026-09-25 | [#595](https://github.com/traverse-framework/registry/pull/595) | unavailable (PR 404s) | escape | contract: `deprecated.json` names a `traverse-contracts` parse failure ([#656](https://github.com/traverse-framework/registry/issues/656)); no `schema` wrapper, no pre/postconditions | `traverse-contracts` parse gate (entry 137) |
| `event.append-plan-create@1.0.0` | 2026-09-15 | [#521](https://github.com/traverse-framework/registry/pull/521) | no | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `inference.ensemble-reconcile@1.0.0` | 2026-09-18 | [#587](https://github.com/traverse-framework/registry/pull/587) | no | escape | contract: `deprecated.json` names a `traverse-contracts` parse failure ([#656](https://github.com/traverse-framework/registry/issues/656)); lifecycle `experimental` | `traverse-contracts` parse gate (entry 137) |
| `inference.score-calibrate@1.0.0` | 2026-09-18 | [#586](https://github.com/traverse-framework/registry/pull/586) | no | escape | contract: `deprecated.json` names a `traverse-contracts` parse failure ([#656](https://github.com/traverse-framework/registry/issues/656)); lifecycle `experimental` | `traverse-contracts` parse gate (entry 137) |
| `model.activation-plan-create@1.0.0` | 2026-09-15 | [#522](https://github.com/traverse-framework/registry/pull/522) | no | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `report.collect-fragments@1.0.0` | 2026-09-09 | [#433](https://github.com/traverse-framework/registry/pull/433) | yes | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `report.enrich-insights@1.0.0` | 2026-09-10 | [#434](https://github.com/traverse-framework/registry/pull/434) | yes | no escape (flag B) | superseded by 1.1.0 for [#441](https://github.com/traverse-framework/registry/issues/441): the chain did not plan in `browserLocalPlan`; each contract was usable alone | n/a |
| `report.enrich-insights@1.1.0` | 2026-09-10 | [#450](https://github.com/traverse-framework/registry/pull/450) | yes | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `report.format@1.0.0` | 2026-09-10 | [#437](https://github.com/traverse-framework/registry/pull/437) | yes | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `report.summarize@1.0.0` | 2026-09-10 | [#435](https://github.com/traverse-framework/registry/pull/435) | yes | no escape (flag B) | superseded by 1.1.0 for [#441](https://github.com/traverse-framework/registry/issues/441), as above | n/a |
| `report.summarize@1.1.0` | 2026-09-10 | [#451](https://github.com/traverse-framework/registry/pull/451) | no | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `report.summarize-semantic@1.0.0` | 2026-09-10 | [#439](https://github.com/traverse-framework/registry/pull/439) | yes | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `report.translate-fr@1.0.0` | 2026-09-10 | [#436](https://github.com/traverse-framework/registry/pull/436) | yes | no escape (flag B) | superseded by 1.1.0 for [#441](https://github.com/traverse-framework/registry/issues/441), as above | n/a |
| `report.translate-fr@1.1.0` | 2026-09-10 | [#452](https://github.com/traverse-framework/registry/pull/452) | yes | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `retention.action-plan-create@1.0.0` | 2026-09-15 | [#523](https://github.com/traverse-framework/registry/pull/523) | no | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `text.detect-language@1.0.0` | 2026-09-11 | [#470](https://github.com/traverse-framework/registry/pull/470) | yes | escape | contract: `deprecated.json` names a `traverse-contracts` parse failure ([#656](https://github.com/traverse-framework/registry/issues/656)); provenance.source `derived` | `traverse-contracts` parse gate (entry 137) |
| `text.truncate@1.0.0` | 2026-09-14 | [#487](https://github.com/traverse-framework/registry/pull/487) | unclear (flag F) | escape (flag A) | behaviour: [#492](https://github.com/traverse-framework/registry/issues/492), schema-legal `max_length` wipes text on wasm32; open, not yet fixed or deprecated, not labeled `bug` | wasm32 execution gate, #509 / [#551](https://github.com/traverse-framework/registry/pull/551) (merged 2026-09-16): the `max_length=2^32` fixture returns `"text":""`, failing `capability.wasm32_budget_wipe_regression` |

#### model-backed (8)

| version | merged | PR | first-pass CI | proposed call | evidence / reason | root-cause gate |
|---|---|---|---|---|---|---|
| `audio.detect-speech-segments@1.0.0` | 2026-09-11 | [#461](https://github.com/traverse-framework/registry/pull/461) | yes | escape | contract: `deprecated.json` names a `traverse-contracts` parse failure ([#656](https://github.com/traverse-framework/registry/issues/656)); `derived` | `traverse-contracts` parse gate (entry 137) |
| `audio.transcribe-speech@1.0.0` | 2026-09-11 | [#474](https://github.com/traverse-framework/registry/pull/474) | yes | escape | behaviour: hallucination on non-speech, fixed by 1.1.0 ([#477](https://github.com/traverse-framework/registry/issues/477)); also contract: `deprecated.json` names a `traverse-contracts` parse failure ([#656](https://github.com/traverse-framework/registry/issues/656)) | none for hallucination (model behaviour); `traverse-contracts` parse gate (entry 137) |
| `audio.transcribe-speech@1.1.0` | 2026-09-12 | [#478](https://github.com/traverse-framework/registry/pull/478) | yes | escape | contract: `deprecated.json` names a `traverse-contracts` parse failure ([#656](https://github.com/traverse-framework/registry/issues/656)); `derived` | `traverse-contracts` parse gate (entry 137) |
| `report.translate-fr-semantic@1.0.0` | 2026-09-10 | [#456](https://github.com/traverse-framework/registry/pull/456) | yes | escape | contract: `deprecated.json` names a `traverse-contracts` parse failure ([#656](https://github.com/traverse-framework/registry/issues/656)); `derived` | `traverse-contracts` parse gate (entry 137) |
| `text.detect-entities@1.0.0` | 2026-09-11 | [#466](https://github.com/traverse-framework/registry/pull/466) | yes | escape | behaviour: WordPiece fragmentation, fixed by 1.1.1 ([#611](https://github.com/traverse-framework/registry/issues/611)); also contract: `deprecated.json` names a `traverse-contracts` parse failure ([#656](https://github.com/traverse-framework/registry/issues/656)) | none for fragmentation (model behaviour); `traverse-contracts` parse gate (entry 137) |
| `text.detect-entities@1.1.1` | 2026-09-28 | [#613](https://github.com/traverse-framework/registry/pull/613) | yes | escape | contract: `deprecated.json` names a `traverse-contracts` parse failure ([#656](https://github.com/traverse-framework/registry/issues/656)); `derived` | `traverse-contracts` parse gate (entry 137) |
| `text.redact-entities@1.0.0` | 2026-09-12 | [#482](https://github.com/traverse-framework/registry/pull/482) | yes | escape | behaviour: shares the #611 fragmentation, fixed by 1.1.1; also contract: `deprecated.json` names a `traverse-contracts` parse failure ([#656](https://github.com/traverse-framework/registry/issues/656)) | none for fragmentation; `traverse-contracts` parse gate (entry 137) |
| `text.redact-entities@1.1.1` | 2026-09-28 | [#613](https://github.com/traverse-framework/registry/pull/613) | yes | escape | contract: `deprecated.json` names a `traverse-contracts` parse failure ([#656](https://github.com/traverse-framework/registry/issues/656)); `derived` | `traverse-contracts` parse gate (entry 137) |

#### other-risk (16)

| version | merged | PR | first-pass CI | proposed call | evidence / reason | root-cause gate |
|---|---|---|---|---|---|---|
| `artifact.recording-finalize@1.0.1` | 2026-09-17 | [#580](https://github.com/traverse-framework/registry/pull/580) | no | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `commerce.cart@1.0.0` | 2026-09-08 | [#402](https://github.com/traverse-framework/registry/pull/402) | no | escape (flag C) | contract: deprecated, string-encoded `lines_json` instead of the structured Wave 2 schema ([#407](https://github.com/traverse-framework/registry/pull/407)) | none named |
| `commerce.cart@1.0.1` | 2026-09-09 | [#407](https://github.com/traverse-framework/registry/pull/407) | yes | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `commerce.pricing-config@1.0.0` | 2026-09-08 | [#402](https://github.com/traverse-framework/registry/pull/402) | no | escape (flag C) | contract: deprecated, string-encoded `rates_json` ([#407](https://github.com/traverse-framework/registry/pull/407)) | none named |
| `commerce.pricing-config@1.0.1` | 2026-09-09 | [#407](https://github.com/traverse-framework/registry/pull/407) | yes | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `core.action-item-store@1.0.0` | 2026-09-08 | [#400](https://github.com/traverse-framework/registry/pull/400) | no | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `core.followup-session@1.0.0` | 2026-09-08 | [#400](https://github.com/traverse-framework/registry/pull/400) | no | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `doc-approval.packet-store@1.0.0` | 2026-09-08 | [#401](https://github.com/traverse-framework/registry/pull/401) | no | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `doc-approval.policy-store@1.0.0` | 2026-09-08 | [#401](https://github.com/traverse-framework/registry/pull/401) | no | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `identity.challenge-session@1.0.0` | 2026-09-08 | [#402](https://github.com/traverse-framework/registry/pull/402) | no | no escape (flag D) | deprecated after a duplicate publish PR overwrote release bytes ([#405](https://github.com/traverse-framework/registry/pull/405)); a release-process failure, not authored content | n/a |
| `identity.challenge-session@1.0.1` | 2026-09-08 | [#405](https://github.com/traverse-framework/registry/pull/405) | yes | escape | contract: `deprecated.json` names a `traverse-contracts` parse failure ([#656](https://github.com/traverse-framework/registry/issues/656)); `registry-wave2` | `traverse-contracts` parse gate (entry 137) |
| `identity.principal-session@1.0.0` | 2026-09-08 | [#402](https://github.com/traverse-framework/registry/pull/402) | no | no escape (flag D) | as above ([#405](https://github.com/traverse-framework/registry/pull/405)) | n/a |
| `identity.principal-session@1.0.1` | 2026-09-08 | [#405](https://github.com/traverse-framework/registry/pull/405) | yes | escape | contract: `deprecated.json` names a `traverse-contracts` parse failure ([#656](https://github.com/traverse-framework/registry/issues/656)); `registry-wave2` | `traverse-contracts` parse gate (entry 137) |
| `record.transition-apply@1.0.1` | 2026-09-17 | [#581](https://github.com/traverse-framework/registry/pull/581) | no | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `support.agent-session@1.0.0` | 2026-09-09 | [#399](https://github.com/traverse-framework/registry/pull/399) | no | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `support.ticket-workspace@1.0.0` | 2026-09-09 | [#399](https://github.com/traverse-framework/registry/pull/399) | no | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |

#### unclassified (12)

| version | merged | PR | first-pass CI | proposed call | evidence / reason | root-cause gate |
|---|---|---|---|---|---|---|
| `audio.calibration-plan-create@1.0.0` | 2026-09-07 | [#369](https://github.com/traverse-framework/registry/pull/369) | no | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `commerce.cart-line-changed@1.0.0` | 2026-09-08 | [#389](https://github.com/traverse-framework/registry/pull/389) | no | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `commerce.order-status-changed@1.0.0` | 2026-09-08 | [#389](https://github.com/traverse-framework/registry/pull/389) | no | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `core.assign-ownership@1.2.0` | 2026-09-08 | [#389](https://github.com/traverse-framework/registry/pull/389) | no | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `core.process-comment@1.2.0` | 2026-09-08 | [#389](https://github.com/traverse-framework/registry/pull/389) | no | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `doc-approval.recommend@1.3.0` | 2026-09-08 | [#389](https://github.com/traverse-framework/registry/pull/389) | no | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `identity.session-revoked@1.0.0` | 2026-09-08 | [#389](https://github.com/traverse-framework/registry/pull/389) | no | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `inference.evidence-normalize@1.0.0` | 2026-09-07 | [#377](https://github.com/traverse-framework/registry/pull/377) | no | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `inference.evidence-normalize@1.0.1` | 2026-09-07 | [#382](https://github.com/traverse-framework/registry/pull/382) | yes | no escape (flag E) | byte-identical WASM to 1.0.0 with no `artifact_reuse` marker, so the unit rule counts it | n/a |
| `platform.decide-state-transition@1.3.0` | 2026-09-08 | [#389](https://github.com/traverse-framework/registry/pull/389) | no | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `support.ticket-sla-breached@1.0.0` | 2026-09-08 | [#389](https://github.com/traverse-framework/registry/pull/389) | no | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |
| `support.ticket-status-changed@1.0.0` | 2026-09-08 | [#389](https://github.com/traverse-framework/registry/pull/389) | no | no escape | no deprecation, `bug` issue, or fixing successor found | n/a |

### Gate rejections by failure code

Across the 43 retrievable publishing PRs (161 pushes):

| failures | code |
|---|---|
| 59 | `signature.missing` |
| 24 | `capability.insufficient_test_coverage` |
| 13 | `inventory.unpublished_capability_unclassified` |
| 12 | `contract.artifact_url_unreachable` |
| 4 | `capability.test_coverage_build_or_test_failed` |
| 3 | `capability.missing_test_coverage_source` |
| 3 | `capabilities.contract_modified` |
| 2 | `contract.missing_artifact_reference` |
| 2 | `contract.missing_licensing` |
| 1 | `contract.missing_authoring_method` |
| 1 | `contract.use_case_persona_ref_unresolvable` |
| 1 | `contract.output_enum_uncovered_by_use_cases` |

Observed while collecting, not reclassified: most `signature.missing` failures
come from the post-merge signing gap that #341 closed, where a PR failed on
records it did not add. `contract.artifact_url_unreachable` failures come from
the first push landing before the release asset was uploaded. Both are
publishing-process failures rather than authored-content failures, but the
pre-registered metric counts every code, so they are reported as they are.

### Totals and bar

First-pass CI is "passed / versions whose first push could be determined"
(flag F rows are left out of the denominator).

| bucket | n | escapes | escape rate | first-pass CI |
|---|---|---|---|---|
| **promotion** | **25** | **7** | **28%** | 15 / 23 (65%) |
| model-backed | 8 | 8 | 100% | 8 / 8 (100%) |
| other risk classes | 16 | 4 | 25% | 4 / 16 (25%) |
| unclassified | 12 | 0 | 0% | 1 / 12 (8%) |

Bar outcome for the promotion bucket: **not met.**

1. n ≥ 20: **met** (n = 25).
2. Escape rate ≤ 10%: **not met** (7 / 25 = 28%). Without flag A it would be
   6 / 25 = 24%, which still fails.
3. Every escape's root cause gated: **met.** Six escapes (all #656 parse
   failures) are now caught by the `traverse-contracts` parse gate (entry 137).
   The `text.truncate@1.0.0` root cause (wasm32 `usize` width, invisible to
   64-bit host tests) is caught by the wasm32 execution gate (#509, #551,
   merged 2026-09-16): run against the 1.0.0 artifact, its `max_length=2^32`
   fixture returns `{"text":"","truncated":true}` where the happy path returns
   `{"text":"hello…","truncated":true}`. That gate is a heuristic (name-matched
   integer fields, emptied strings only); generalising it is tracked
   separately. Corrected 2026-11-10 (decision-log entry 142); the results
   first reported this condition as not met. The bar outcome is unchanged,
   because condition 2 fails.

Six of the seven promotion-bucket escapes are a single failure class: contract
values (lifecycle `experimental`, provenance `derived`, the missing `schema`
wrapper) that the registry's own CI accepted but Traverse's parser rejected.
That gate now exists. One escape is a behaviour defect.

Rows changed by the owner: **none.**
