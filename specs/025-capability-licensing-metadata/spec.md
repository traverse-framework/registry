# Feature Specification: Capability Licensing Metadata

**Feature Branch**: `claude/025-capability-licensing-metadata`
**Created**: 2026-09-16
**Status**: Approved (2026-09-16, v1.0.0)
**Input**: Decided via `/brainstorm` with the repo owner (machine-readable
licensing and usage rights for published capabilities). Full reasoning, all
thirteen questions with options and rationale: `docs/decision-log.md`
entry 116. That decision-log entry is also the ADR-class record for this
work — this repo keeps ADR rationale in the decision log
(`docs/decision-log.md` entry 75); it has no separate `docs/adr/`.

## Purpose

The Registry publishes executable WASM capabilities, but consumers cannot
reliably discover from the capability index whether an entry may be used or
redistributed commercially. Treating a capability’s source license as if it
also covered bundled or referenced models, datasets, or generated artifacts
creates unsafe and ambiguous reuse decisions.

This spec adds an additive, machine-readable `licensing` object to capability
contracts, validates it in CI, and projects normalized rights fields into the
public index. It is a **discoverability and governance** surface: it does
**not** make the Registry a source of legal advice, and a Registry signature
authenticates the published declaration, not the truth of the claim.

## Scope

In scope:

- Optional top-level `licensing` on `capabilities/**/contract.json` (required
  for newly published versions only after a later activation change governed
  by this spec’s FR-010)
- Flat lean field shape (one SPDX expression + explicit rights enums +
  attribution + verification)
- CI validation of shape, enums, SPDX expressions (pinned
  `license-expression`), `LicenseRef-*` evidence rules, and a tiny
  hard-contradiction table
- Index projection of normalized filter fields; absent/`unknown` when the
  contract omits `licensing`
- Explicit non-inheritance: capability `licensing` MUST NOT be interpreted as
  rights for models, datasets, or other dependencies

Out of scope:

- Nested `code` vs `artifact` license objects (deferred until a real
  divergent case appears)
- `verification.status` values other than `maintainer-declared` (reserved;
  rejected until a real review process exists)
- Traverse CLI search/inspect filters (cross-repo follow-up)
- Model-package / dataset licensing schema (related gap; separate surface)
- Rewriting or backfilling immutable legacy contracts
- Deriving `commercial_use` / `redistribution` from SPDX by general legal
  inference
- Blocking publication solely because a license is restrictive

## Design Decisions

### Flat `licensing` object

```json
"licensing": {
  "spdx_expression": "MIT",
  "commercial_use": "allowed",
  "redistribution": "allowed",
  "attribution_required": true,
  "license_files": ["LICENSE"],
  "source_url": "https://github.com/example/project",
  "verification": {
    "status": "maintainer-declared",
    "evidence_url": "https://github.com/example/project/blob/main/LICENSE",
    "reviewed_at": "2026-09-16"
  }
}
```

When `licensing` is present, these fields are **required**:
`spdx_expression`, `commercial_use`, `redistribution`, `attribution_required`,
`verification.status`. Optional: `license_files`, `source_url`,
`verification.evidence_url`, `verification.reviewed_at`.

### Rights enumerations

`commercial_use` and `redistribution` MUST be exactly one of:

- `allowed`
- `forbidden`
- `conditional`
- `unknown`

Consumers and policy engines MUST NOT treat `conditional` or `unknown` as
permission. Missing legacy metadata MUST be indexed as `unknown`, never as
`allowed`.

### Verification

v1 accepts only `verification.status: "maintainer-declared"`. Values
`registry-reviewed` and `verified-with-evidence` are reserved and MUST be
rejected by CI until a later amendment defines a real review process.

### SPDX

CI MUST validate `spdx_expression` with a pinned version of the PyPA
`license-expression` library. The pin MUST be recorded in this spec’s
implementation notes / validation environment when landed. Custom licenses
use `LicenseRef-*` and MUST include `verification.evidence_url` or
`license_files`.

### Hard contradictions

CI MUST reject a documented, tiny set of unambiguous contradictions (for
example `UNLICENSED` or equivalent non-redistributable markers paired with
`redistribution: "allowed"`). CI MUST NOT attempt general legal determination
from license names alone.

### Capability vs dependency rights

The `licensing` block describes the capability implementation and executable
artifact only. It MUST NOT imply rights for external models, datasets, label
sets, or other dependencies. Those remain on their own manifests/surfaces.

### Activation

On first merge of the implementing change, `licensing` is **optional**. A
later activation PR (same governing spec) makes it **required** for newly
ADDED capability versions after the activation date. Existing immutable
contracts are never rewritten.

## Functional Requirements

- **FR-001**: Capability contracts MAY include a top-level `licensing` object
  with the flat shape defined above. Additional nested `code`/`artifact`
  license objects are out of scope for v1.0.0.
- **FR-002**: When `licensing` is present, CI MUST require
  `spdx_expression` (string), `commercial_use` and `redistribution` (rights
  enums), `attribution_required` (boolean), and `verification.status`
  exactly equal to `maintainer-declared`.
- **FR-003**: CI MUST validate `spdx_expression` using a pinned
  `license-expression` version. `LicenseRef-*` expressions MUST also provide
  `verification.evidence_url` or a non-empty `license_files` array.
- **FR-004**: `commercial_use` and `redistribution` MUST be one of
  `allowed` | `forbidden` | `conditional` | `unknown`. Other values MUST fail
  validation.
- **FR-005**: CI MUST reject malformed `source_url` / `evidence_url` values
  (non-HTTPS URL, credentials, or host filesystem paths). Evidence fields
  MUST NOT contain secrets.
- **FR-006**: CI MUST reject entries matching the documented hard-contradiction
  table. CI MUST NOT otherwise infer rights from SPDX identifiers.
- **FR-007**: `licensing` metadata is publisher-declared input. Registry
  signatures authenticate the published bytes, not the correctness of the
  legal claim. Documentation MUST state that the field is advisory and not a
  legal certification.
- **FR-008**: Capability `licensing` MUST NOT be treated as granting or
  denying rights for model/dataset/dependency artifacts. Index and docs MUST
  preserve that distinction.
- **FR-009**: `scripts/ci/build_index.py` MUST project, for each capability
  entry: `license_expression`, `commercial_use`, `redistribution`, and
  `verification_status`. When the contract omits `licensing`, each of
  `commercial_use`, `redistribution`, and `verification_status` MUST be
  `unknown`, and `license_expression` MUST be absent or null (implementation
  picks one consistently and documents it). Projected values MUST match the
  contract when present.
- **FR-010**: Until an activation change merges, `licensing` remains optional
  for new publishes. After activation, every newly ADDED
  `capabilities/**/contract.json` version MUST include a valid `licensing`
  object. The activation change MUST record its effective date in release
  notes / decision log. Already-published versions remain valid without the
  field.
- **FR-011**: Restrictive licenses MUST NOT by themselves block publication;
  the restriction MUST be visible and machine-readable.
- **FR-012**: Contract signing / publication evidence (where applicable) MUST
  cover the `licensing` bytes as part of the signed contract content — no
  separate unsigned sidecar for rights claims.

## Success Criteria

- **SC-001**: A contract with a well-formed `licensing` object passes
  validation; malformed enums, bad SPDX, missing required subfields, bad
  URLs, reserved verification statuses, and hard contradictions fail with
  stable error codes.
- **SC-002**: Fixtures cover `allowed`, `forbidden`, `conditional`,
  `unknown`, SPDX expressions, `LicenseRef-*` with and without evidence,
  malformed, and contradictory cases.
- **SC-003**: Generated index fields match the contract; legacy contracts
  without `licensing` index as `unknown` for rights fields.
- **SC-004**: Existing published capabilities continue to pass validation
  unchanged before activation.
- **SC-005**: After activation, a newly added capability version without
  `licensing` fails CI; legacy versions without it still pass.
- **SC-006**: Docs state that licensing metadata is declarative, signed with
  the contract, and not a legal certification; capability rights do not
  inherit to models/datasets.

## Governing Relationship

- Additive to `specs/001-registry-foundation` (contract record shape) and
  `specs/002-capability-validation` / `specs/003-index-release-pipeline`
  (validation and index build). Those specs remain unmodified.
- Implementation tickets cite `- 025-capability-licensing-metadata` under
  `## Governing Spec`.
- Cross-repo CLI filtering is out of scope here and MUST NOT be required to
  close this spec’s Registry-side success criteria.
