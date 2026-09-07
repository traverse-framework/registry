# Publication Lifecycle Report

**Status: no edition published — `n = 0`.**

This document is the periodic, hand-written report on capability publication
friction for `traverse-framework/registry`, derived from the opt-in aggregate
records under [`metrics/publication-lifecycle/`](../metrics/publication-lifecycle/).
It is governed by
[`specs/022-publication-lifecycle-measurement`](../specs/022-publication-lifecycle-measurement/spec.md)
and was designed in `docs/decision-log.md` entry 83 (resolving `registry#354`).

The first edition will be written once **at least 20** records exist; a new
edition follows for each additional 20. As of now there are **0** records, so
there is nothing to report yet — this file exists so the methodology is fixed
in advance of any data, not selected after seeing it.

---

## Methodology (fixed in advance)

### What is measured

Per contributed PR: elapsed wall-clock hours from open to merge/close, the
number of submitted reviews, the count of CI validation failures by stable
category, and a deterministic `simple`/`complex` complexity bucket (with any
author override and its reason). Nothing else. No capability identity, no
author identity, no timestamp finer than the calendar month. See the record
[schema](../metrics/publication-lifecycle/schema.json) and
`specs/022-publication-lifecycle-measurement` FR-001–FR-010.

The clock starts at **PR open**. Time spent authoring the contract, source,
and tests *before* the PR was opened is not captured by this report; adding it
would require author-side or `traverse-cli` instrumentation and a separate
spec amendment (decision-log entry 83, Q4).

### Sample and selection bias

Records are **opt-in and self-selected**. An author chooses whether to run the
export helper and open a PR adding the record. This is not a random or
complete sample of publications, and it never will be:

- Authors who had a smooth publication may be less motivated to file a record
  than authors who hit friction — or the reverse. The direction of this bias
  is unknown and may shift over time.
- Publications by teams that do not adopt the practice are absent entirely.
- A `closed_unmerged` outcome is the least likely to be recorded, since the
  author has the least incentive to return to it.

Every edition of this report MUST state the record count, MUST NOT extrapolate
from it to "the publication experience" in general, and MUST describe the
sample as self-selected opt-in whenever it presents a statistic.

### Statistics reported per edition

For each of the `simple` and `complex` buckets, separately:

- record count `n`
- median and 90th-percentile `hours_open_to_conclusion`
- median `review_rounds`
- frequency of each `ci_failure_counts` category (share of records in the
  bucket that hit that category at least once, and total occurrences)

No mean is reported (small `n`, skewed distributions). No per-capability or
per-author breakdown is possible from the records and none is produced
(spec FR-018).

### Uncertainty

With small `n`, medians and percentiles are unstable. Each edition MUST:

- report `n` alongside every figure;
- give the min–max range, not just the point estimate, for any bucket with
  `n < 30`;
- explicitly decline to compare `simple` vs `complex` when either bucket has
  `n < 10`, stating that instead of showing a comparison the data cannot
  support.

### Data retention

Records are immutable and retained indefinitely — they are tiny, already
redacted, and their value is longitudinal. Each edition of this report
supersedes the previous one **in place** (this file is overwritten); prior
editions remain available in this repo's git history. No separate archival
copies are kept.

### What the report is not

It is advisory prose for humans deciding where publication governance is worth
its cost. Nothing automated consumes it: not a CI gate, not the resolver, not
the catalog, not any approval step. It creates no ranking and no
fast-track/auto-approval mechanism (spec FR-018).

---

## Editions

_None yet._
