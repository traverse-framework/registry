# ECCA Capability Migration Report

Governing spec: Spec 534 FR-020. Tracking issue: `traverse-framework/registry#170`.

## Scope

All 11 unique capability ids currently published in `capabilities/*/*/*/contract.json`
(49 published versions total, spanning every historical version of each id).

## Outcome

| Outcome | Count |
|---|---|
| `no-event-required` (evidence-backed) | 11 |
| `governed-event-declared` | 0 |
| Blocked | 0 |
| Exceptions | 0 |

Every one of the 11 capabilities has `side_effects: [memory_only]` and empty
`emits`/`consumes` on every published version — each is a pure, deterministic
transformation (validation, formatting, analysis, recommendation, or
summarization) that returns its result directly to the caller. None has an
externally meaningful asynchronous effect, so none requires a governed event
product. No capability was forced to declare an artificial event to satisfy
a quota (Spec 534 QG-004) — this report is the evidence that conclusion is
warranted, not assumed.

Full per-capability evidence: `contracts/governance/ecca-capability-inventory.json`.

## Relationship to `traverse#899`

`traverse-framework/traverse#899` ("Inventory published capabilities for
ECCA event-product compliance") is closed and claims "every currently
published capability" was inventoried. Its actual coverage
(`contracts/governance/ecca-capability-inventory.json` in that repo) is
scoped to that repository's own `contracts/examples/`/`contracts/inference/`
fixture tree — a different, unrelated set of artifacts from this registry's
real `capabilities/` tree, despite several `capability_id` values
coincidentally matching (`doc-approval.analyze`, `meeting-notes.process`,
etc. exist as separate content in both places). `#899` does not satisfy
FR-020 for this registry's actual catalog. This report and its companion
inventory are what does.

## Regression coverage

No new capability may be republished under a changed emits/consumes
declaration without going through the existing `capability_validation.py`
gate and, once a real event product exists, `validate_event_product_descriptor`.
No enforcement code changes were needed for this inventory pass itself,
since every capability here has zero declared events to validate.
