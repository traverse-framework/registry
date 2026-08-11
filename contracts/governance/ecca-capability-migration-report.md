# ECCA Capability Migration Report

Governing spec: Spec 534 FR-020. Tracking issue: `traverse-framework/registry#170`
(completeness gate: `traverse-framework/registry#253`).

## Scope

All 28 unique capability ids currently published in `capabilities/*/*/*/contract.json`
(97 published versions total, spanning every historical version of each id).

## Outcome

| Outcome | Count |
|---|---|
| `no-event-required` (evidence-backed) | 27 |
| `governed-event-declared` | 1 |
| Blocked | 0 |
| Exceptions | 0 |

Most capabilities have empty `emits`/`consumes` and return results synchronously to
the caller, so they classify `no-event-required` with evidence. The Loop capability
`core.transition-action-status` declares governed event
`core.action-item.status-transitioned@1.0.0` (`governed-event-declared`).

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

`scripts/ci/capability_validation.py` runs
`check_ecca_capability_inventory_coverage` on every CI pass. A published
capability without an inventory entry fails the gate — inventory writes
must not be skipped or patched around.
