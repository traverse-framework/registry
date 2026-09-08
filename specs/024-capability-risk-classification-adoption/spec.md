# Feature Specification: Capability Risk Classification Adoption

**Feature Branch**: `024-capability-risk-classification-adoption`
**Created**: 2026-09-08
**Status**: Approved (2026-09-08, v1.0.0)
**Input**: Registry-side adoption of `traverse-framework/traverse`'s `Spec 109` ("Runtime Workflow Proposals and Authorization", Approved) FR-005 and FR-006, to unblock `traverse-framework/registry#384` (and, downstream, `traverse#1150` / discussion `#1102`: a browser demo that selects capabilities at runtime from the live registry and must constrain selection to what is safe to run unattended). The behavior lands in this repo's publish gate (`scripts/ci/capability_validation.py`) and its public metadata surfaces (`catalog/catalog.json`, `index.json`) plus a small `crates/traverse-registry/` helper; a dedicated registry spec is required per `013-inherited-registry-governance` FR-002.

**Approval note**: the repo owner approved this adoption directly and standalone in the `registry-ops` `/brainstorm` session on 2026-09-08 (`docs/decision-log.md` entry 92), grounded in the already-owner-approved `traverse` Spec 109. Registered straight into `specs[]` with `status: approved` (no `draft_specs[]` detour), the same path `021-host-load-trust-boundary-adoption` and `016-ecca-event-product-adoption` v2.1.0 used.

## Why This Is An Adoption, Not A Fresh Spec

`traverse` Spec 109 is owner-authored and approved in `traverse-framework/traverse`, and its risk-classification model is already implemented in the `traverse-contracts` crate: `RiskMetadata`, the `EffectClass` / `DeterminismClass` / `EgressPolicy` enums, `is_automatic_eligible()` (the single canonical FR-006 derivation), and `default_risk_metadata()` (the conservative pre-109 fallback). This spec does not re-litigate any of that and does not re-implement the derivation. It translates the subset that is registry behavior -- what a published contract MUST declare, and what this registry's public metadata surfaces expose -- into this repo's own spec-numbered governance, the same way `016` translated `traverse` Spec 534 and `021` translated `traverse` Spec 127.

The parts of Spec 109 that are `traverse` concerns stay in `traverse` under Spec 109: the MCP proposal lifecycle, approval tokens, the runtime's automatic-vs-token gate, manifest-side risk narrowing (`ManifestRiskPolicy`), and field-level data-flow enforcement at mapping time (FR-011).

## Purpose

Make the structured risk classification `traverse` Spec 109 FR-005 requires a
first-class, validated part of a published capability contract, and expose it --
together with a computed automatic-eligibility verdict -- on this registry's
public discovery surfaces, so a consumer can constrain runtime capability
selection to the safe-to-run-unattended subset without fetching and
interpreting every full contract itself.

## Requirements

- **FR-001**: A newly added or modified `capabilities/<namespace>/<id>/<version>/contract.json`
  MUST declare a top-level `risk` object that deserializes as
  `traverse-contracts::RiskMetadata`:
  - `effect_class` (required): one of `pure_read`, `state_write`,
    `external_effect`, `irreversible_effect`.
  - `determinism_class` (required): one of `deterministic`,
    `externally_variable`, `model_derived`.
  - `reliability` (required): an object with boolean `idempotency_required`,
    `retryable`, and `compensation_available`.
  - `data_flow` (optional): when present, an object with
    `accepted_data_classifications`, `produced_data_classifications` (each a
    list of `{field_path, classification}` where `classification` is one of
    `public`, `internal`, `confidential`, `restricted`), and `egress_policy`
    (either `"denied"` or `{"allowed_connectors": [<string>, ...]}`). When
    absent it defaults, per `traverse-contracts`, to no classifications and
    `egress_policy: denied`.
  This is enforced diff-based (added/modified paths only). Immutable contracts
  published before this spec are never retro-flagged (`001` FR immutability;
  precedent: `023-authoring-assurance` / decision-log entry 87).

- **FR-002**: `scripts/ci/capability_validation.py` MUST reject a `risk` block
  that is missing a required field (`contract.missing_risk_metadata`) or whose
  enum value is outside the FR-001 vocabularies / whose shape does not
  deserialize (`contract.invalid_risk_metadata`). A well-formed `risk` block
  whose classes simply make the capability not automatic-eligible is valid --
  this gate checks shape and vocabulary, not the verdict.

- **FR-003**: The public discovery catalog (`catalog/catalog.json`) and the
  aggregated release index (`index.json`) MUST expose, for every active
  (non-deprecated) published capability version, over the same CORS/no-auth
  terms as the rest of each surface:
  - `risk`: the `RiskMetadata` object -- as declared by the contract, or the
    `traverse-contracts::default_risk_metadata()` value when the contract
    declares none.
  - `is_automatic_eligible`: a boolean, the result of
    `traverse-contracts::is_automatic_eligible` applied to that `risk`.
  - `risk_source`: `"declared"` when the value came from the contract,
    `"conservative_default"` when it came from `default_risk_metadata()`.

- **FR-004**: The `is_automatic_eligible` verdict and the missing-`risk`
  fallback MUST be computed through `traverse-contracts` (the crate that owns
  Spec 109's derivation) -- never re-implemented in the catalog pipeline's
  Python or in the `no_std` `catalog-builder` wasm. A `crates/traverse-registry/`
  binary provides the computation; `scripts/ci/gather_catalog_data.py` invokes
  it and `scripts/ci/build_index.py` consumes the same computation, so the two
  surfaces cannot disagree and neither drifts if `traverse-contracts` changes
  the rule.

- **FR-005**: Every change under this spec MUST be additive. No existing
  `catalog.json` or `index.json` field changes meaning; consumers that ignore
  the new fields are unaffected.

- **FR-006**: For the registry-owned portion, behavior MUST match `traverse`
  Spec 109 FR-005 and FR-006 and the `traverse-contracts` implementation of
  `RiskMetadata` / `is_automatic_eligible` / `default_risk_metadata`. Where they
  diverge, `traverse` Spec 109 and `traverse-contracts` are authoritative and
  this spec is amended to match.

## Success Criteria

- **SC-001**: A PR adding a new `contract.json` with no `risk` block fails
  `capability-validation` with `contract.missing_risk_metadata`; adding a
  well-formed `risk` block clears it.
- **SC-002**: A PR whose new contract sets `effect_class: "read_only"` (not a
  Spec 109 vocabulary value) fails with `contract.invalid_risk_metadata`.
- **SC-003**: After a catalog build, a pre-024 immutable capability version
  (e.g. `core/core.calculate-price@1.1.0`) appears in `catalog.json` and
  `index.json` with `risk_source: "conservative_default"`,
  `is_automatic_eligible: false`, and a `risk` object equal to
  `default_risk_metadata()`.
- **SC-004**: A capability whose contract declares
  `effect_class: pure_read`, `determinism_class: deterministic`,
  `data_flow.egress_policy: denied`, and `reliability.idempotency_required: false`
  appears with `risk_source: "declared"` and `is_automatic_eligible: true`;
  flipping any one of those makes `is_automatic_eligible` `false` with no
  pipeline code change.
- **SC-005**: A consumer can filter `catalog.json` (or `index.json`) to
  `is_automatic_eligible == true` with no null-handling and no `traverse-cli`
  call, and the result excludes every legacy version.

## Governing Relationship

Layered on the existing capability-contract governance (`001-registry-foundation`,
`006-public-scope-and-identity`, `023-authoring-assurance`) as a new dedicated
spec per `013-inherited-registry-governance` FR-002. Adopts `traverse` Spec 109
FR-005/FR-006; does not supersede any existing registry spec. Coexists with the
`#383` per-capability contract mirror (decision-log entry 90): a mirrored
`contract.json` carries whatever `risk` block the contract declares, verbatim.

## Out of Scope

- Field-level `data_flow.field_path` resolution against `inputs.schema` /
  `outputs.schema` (RFC 6901 pointers, `traverse` Spec 109 FR-011) -- an
  additive tightening of FR-002, not required for the FR-006 eligibility
  verdict; a follow-up.
- Backfilling `risk` onto contracts published before this spec -- structurally
  impossible (immutability); they resolve via `conservative_default`.
- `traverse-cli capability publish` emitting `risk` so publishers need not
  hand-author it -- a `traverse` cross-repo follow-up, same class as decision
  55 / decision 87's `authoring.method` gap.
- Manifest-side risk narrowing, approval tokens, and the runtime's
  automatic-execution gate -- `traverse` Spec 109 concerns.
- Workflows and event products (`workflows/`, `events/`) -- Spec 109 FR-005 is
  about capability contracts.
