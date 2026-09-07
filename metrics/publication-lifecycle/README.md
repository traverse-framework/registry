# Publication lifecycle measurement records

Opt-in, aggregate, redacted records of how much friction each capability
publication PR against this repo actually carried. Governed by
[`specs/022-publication-lifecycle-measurement`](../../specs/022-publication-lifecycle-measurement/spec.md);
designed in `docs/decision-log.md` entry 83, resolving `registry#354`.

## What a record is

One JSON file per contributed PR at `pr-<n>.json`, conforming to
[`schema.json`](schema.json). It carries only:

| field | meaning |
|---|---|
| `pr_number` | the PR this measures |
| `period` | calendar month (`YYYY-MM`) the PR merged or closed — **month precision only** |
| `outcome` | `merged` or `closed_unmerged` |
| `hours_open_to_conclusion` | whole hours, PR open → merge/close |
| `review_rounds` | count of submitted reviews |
| `complexity` | `simple`/`complex` bucket: the rubric `computed` value, the `final` value, and — if an author overrode it — `override: true` with a ≤200-char `override_reason` |
| `rubric` | the six deterministic signals behind `computed` (see the spec, FR-008) |
| `ci_failure_counts` | how many CI validation failures the PR hit, by stable category (FR-010) |

There is **no** capability id, namespace, owner, author, or wall-clock
timestamp — by construction, and enforced by
`scripts/ci/measurement_validation.py`. A record can be traced to a PR (and
thus, by hand, to a capability), but the records cannot be bulk-sorted into a
per-capability or per-author ranking. That is deliberate (spec FR-018).

## Contributing a record

Records are **opt-in and default off**: one exists only because an author
chose to generate and contribute it.

```bash
# after your publication PR has merged (or been closed):
scripts/metrics/publication_lifecycle_export.py --pr <n>

# if the computed simple/complex bucket is wrong for your PR, override it —
# the override and your reason are themselves recorded as evidence:
scripts/metrics/publication_lifecycle_export.py --pr <n> \
    --set-complexity complex --reason "small schema, but the reconciliation logic is subtle"
```

Review the generated `pr-<n>.json`, then open a normal PR adding it. CI runs
`scripts/ci/measurement_validation.py` over it (schema + redaction + rubric
consistency). Populate `ci_failure_counts` by hand from your PR's own CI
history if the export left it empty and your PR did hit failures.

## Immutability

A merged `pr-<n>.json` is never edited or deleted — same rule as a published
`contract.json`. If a record was wrong, that is itself data; leave it.

## The report

`docs/publication-lifecycle-report.md` is the periodic, hand-written report
derived from these records. The first edition is published once at least 20
records exist. No automated system — no CI gate, resolver, catalog, or
approval step — reads these records or that report.
