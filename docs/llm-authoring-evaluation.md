# LLM-Assisted Authoring Evaluation

**Status: methodology fixed, no results yet.**

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

Not yet collected.
