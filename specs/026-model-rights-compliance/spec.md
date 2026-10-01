# Feature Specification: Model Rights Compliance

**Feature Branch**: `claude/model-license-registry-contract-907e5f`
**Created**: 2026-09-30
**Status**: Approved (2026-09-30, v1.0.0)
**Input**: App-developer request for a model-license compliance contract,
decided via `/brainstorm` with the repo owner. Full reasoning, all fourteen
questions with options and rationale: `docs/decision-log.md` entry 127, which
is also the ADR-class record for this work (decision-log entry 75).

## Purpose

Model-backed capabilities (spec 001 FR-017, "agents") embed third-party model
weights in WASM artifacts that this Registry hosts and redistributes publicly.
Today each `ai.models` `ModelRef` records only an SPDX expression, an
attribution flag, and a pinned upstream revision. Consumers cannot discover
from the contract or index whether a model may be redistributed, used
commercially, or modified; what NOTICE content must travel with it; how the
shipped weights differ from upstream; or whether the rights record itself is
authenticated.

This spec extends `ModelRef` into a complete, machine-readable, signed,
fail-closed model rights record. Like spec 025, it is a **discoverability and
governance** surface, not legal advice: a Registry signature authenticates the
published declaration, not the truth of the legal claim.

## Scope

In scope:

- Additional rights, evidence, derivation and data-obligation fields on
  object-shaped `ai.models` entries (spec 001 FR-017 `ModelRef`)
- CI validation, including fail-closed `unknown`, hard contradictions, pinned
  file digests, and rights-drift detection
- Contract-hash signing in new `signature.json` files
- An additive `revoked.json` lifecycle marker
- Index projection (including derived `usage_class`) and round-trip tests
- `traverse-registry` crate types for every field above
- Publisher and consumer documentation, plus an example fixture

Out of scope:

- A first-class `models/` record type or standalone model packages
- Any Registry legal review; `verification.status` values other than
  `maintainer-declared` remain reserved (as in spec 025)
- Required live Hugging Face (or other upstream) access in CI
- Breaking consumers pinned to an exact version (spec 005 SC-005 holds)
- Inferring any right from an SPDX identifier or from other fields
- Rewriting existing immutable contracts or `signature.json` files
- Traverse CLI inspection and runtime enforcement (cross-repo follow-up)

## Design Decisions

### Extended `ModelRef`

```json
{
  "id": "dslim/distilbert-NER",
  "huggingface_id": "dslim/distilbert-NER",
  "revision": "dfa2838a127384aabb82ed7719e16dab84c42a2a",
  "spdx_expression": "Apache-2.0",
  "attribution_required": true,
  "copyright": "Copyright holders of dslim/distilbert-NER",
  "commercial_use": "allowed",
  "redistribution": "allowed",
  "derivatives": "allowed",
  "license_files": [
    { "url": "https://github.com/traverse-framework/registry/releases/download/artifacts/<tag>/LICENSE", "sha256": "<64 hex>" }
  ],
  "notice_files": [
    { "url": "https://github.com/traverse-framework/registry/releases/download/artifacts/<tag>/NOTICE", "sha256": "<64 hex>" }
  ],
  "derivation": {
    "transformations": ["quantize", "format-convert"],
    "description": "int8 per-row quantization of all Linear weights; packed into a flat .bin layout",
    "tool_url": "https://github.com/traverse-framework/registry/tree/<commit>/capability-src/text-detect-entities/data",
    "converted_sha256": "<64 hex>"
  },
  "data_obligations": [
    {
      "dataset": "CoNLL-2003",
      "kind": "labels",
      "obligation": "Research-use terms of the Reuters corpus apply to the labelled data",
      "source_url": "https://www.clips.uantwerpen.be/conll2003/ner/"
    }
  ],
  "verification": { "status": "maintainer-declared", "evidence_url": "https://huggingface.co/dslim/distilbert-NER/blob/<revision>/README.md" },
  "rights_change": { "reason": "...", "evidence_url": "https://..." }
}
```

`rights_change` is present only when Q13 drift applies (FR-011).

### Rights enumerations

`commercial_use`, `redistribution` and `derivatives` use spec 025's enum:
`allowed` | `forbidden` | `conditional` | `unknown`. On a model reference,
`unknown` **fails validation**: the Registry redistributes these weights, so
it must not do so on unknown rights. This is intentionally stricter than spec
025's capability-level rule, which is unchanged.

### Hard contradictions (model-specific)

- `redistribution: "forbidden"` — the Registry publicly hosts the artifact
  that embeds the weights, so publishing is redistribution.
- `derivatives: "forbidden"` together with a `derivation` object.
- Spec 025's non-redistributable SPDX markers (`UNLICENSED`,
  `LicenseRef-Proprietary`, …) with `redistribution: "allowed"`.

CI MUST NOT otherwise infer rights from SPDX identifiers.

### Pinned evidence files

`license_files[]` and `notice_files[]` entries are `{url, sha256}`. URLs MUST
be this Registry's GitHub Release assets; CI fetches each and verifies the
digest (same posture as `check_new_contract_artifact_fetchable`). No upstream
host is contacted.

### Derived usage class

The index projects `usage_class` per model reference, computed only from the
rights enums: `unrestricted` (all `allowed`), `evaluation-only`
(`commercial_use: "forbidden"`), otherwise `conditional`. It is a projection,
never a contract field.

### Contract signing

`signature.json` files written after this spec's signing change add
`contract_sha256` and `contract_signature_hex`: an Ed25519 signature, by the
same key as `public_key_hex` / `catalog/signing-key.pub`, over the SHA-256 of
the contract's canonical JSON (UTF-8, keys sorted, no insignificant
whitespace). This authenticates every rights field, NOTICE digest and
derivation record, and is what satisfies spec 025 FR-012. Existing
`signature.json` files are immutable and are not re-signed.

### Revocation

A sibling `revoked.json` (`{reason, evidence_url, revoked_at}`) is additive,
like spec 005's `deprecated.json`. The contract and artifact stay untouched.

## Functional Requirements

- **FR-001**: A newly ADDED contract with `ai.model_backed: true` MUST, for
  every `ai.models` entry, provide `commercial_use`, `redistribution`,
  `derivatives`, `license_files` (non-empty), and `verification.status`
  equal to `maintainer-declared`, in addition to spec 001 FR-017's required
  `ModelRef` fields. `notice_files` is REQUIRED when `attribution_required`
  is `true`.
- **FR-002**: Rights enums MUST be one of `allowed|forbidden|conditional|unknown`;
  `unknown` MUST fail validation on a model reference.
- **FR-003**: Non-SPDX licenses MUST use `LicenseRef-*` with
  `verification.evidence_url` and a `license_files` entry (spec 025 FR-003
  parity, with the file now required).
- **FR-004**: CI MUST reject the hard contradictions listed above, and
  nothing beyond that table on SPDX grounds.
- **FR-005**: Each `license_files` / `notice_files` URL MUST be a Registry
  GitHub Release asset; CI MUST fetch it and fail on a digest mismatch or
  fetch failure.
- **FR-006**: When the shipped weights differ from the pinned upstream
  revision, `derivation` MUST be present with a non-empty `transformations`
  array from `quantize|format-convert|prune|distill|fine-tune|other`, a
  non-empty `description`, and `converted_sha256`. `converted_sha256` MUST
  equal a `sha256` listed in the capability crate's `model-weights.json`.
- **FR-007**: `data_obligations`, when present, MUST be an array of
  `{dataset, kind, obligation, source_url}` with optional `spdx_expression`
  (validated as SPDX) and `kind` in `training|labels|eval`. Absence means
  "not stated by the publisher", never "none"; docs and index MUST preserve
  that distinction.
- **FR-008**: URLs anywhere in a model reference MUST be HTTPS with no
  credentials or host filesystem paths (spec 025 FR-005 parity). Upstream
  `revision` MUST be an immutable commit identifier, not a branch or tag.
- **FR-009**: `verification.status` values other than `maintainer-declared`
  MUST be rejected (reserved, as in spec 025).
- **FR-010**: Validation MUST NOT accept a model reference where any field
  above is malformed; error codes MUST be stable and documented.
- **FR-011**: For a newly ADDED contract, CI MUST compare each model
  reference against (a) the latest prior version of the same capability and
  (b) every other published contract citing the same model `id` and pinned
  `revision`/`source_url`. Any difference in `spdx_expression`, the three
  rights enums, or `attribution_required` MUST fail unless the new reference
  carries `rights_change: {reason, evidence_url}`.
- **FR-012**: `signature.json` files written after the signing change MUST
  include `contract_sha256` and `contract_signature_hex` as defined above; CI
  MUST verify both for any signature that has them. Existing signatures stay
  valid unchanged.
- **FR-013**: A `revoked.json` sibling MUST be additive; CI MUST reject one
  that modifies or removes the contract or artifact reference. The index MUST
  keep a revoked entry with `status: "revoked"`, its revocation record, and
  its full rights record; a revoked version MUST NOT be presented as active.
- **FR-014**: `scripts/ci/build_index.py` MUST project every model reference
  field defined here verbatim plus the derived `usage_class`, and an entry
  `status` of `active` | `deprecated` | `revoked`. Round-trip tests MUST prove
  no rights field is dropped or altered.
- **FR-015**: The `traverse-registry` crate MUST model every field and status
  above, with tests, so that crate consumers see the same contract CI
  enforces.
- **FR-016**: Docs MUST describe publisher obligations (what to declare,
  where to host evidence, when `rights_change` is needed) and consumer
  obligations (deny-by-default on `conditional`, honoring `revoked`, shipping
  NOTICE content), and state that the record is declarative and signed, not a
  legal certification.
- **FR-017**: Already-published versions are never retroactively judged by
  this spec. The five existing agents are brought into compliance by
  publishing new MINOR versions reusing their predecessor artifacts.

## Success Criteria

- **SC-001**: Fixtures for permissive, non-commercial (`evaluation-only`),
  conditional/restricted, deprecated, and revoked model-backed contracts
  validate and index as expected.
- **SC-002**: Negative fixtures fail with stable codes for: missing rights
  field, `unknown` rights, malformed enum, `LicenseRef-*` without evidence,
  missing NOTICE when attribution is required, digest mismatch on a license or
  NOTICE file, mutable revision, missing/invalid `derivation`, each hard
  contradiction, and unacknowledged rights drift (both version and
  cross-capability).
- **SC-003**: Signature tests prove a contract-hash signature verifies, and
  that changing any rights byte breaks it.
- **SC-004**: Index round-trip tests prove every model reference field and
  `status` survive projection unchanged.
- **SC-005**: An example fixture contains LICENSE, NOTICE, attribution,
  provenance, derivation and data-obligation metadata and passes all gates.
- **SC-006**: All existing published contracts still pass validation
  unchanged.

## Governing Relationship

- Additive to spec 001 FR-017 (`ModelRef` shape), spec 005 (lifecycle
  markers), spec 007 (artifact signing) and spec 025 (licensing). None of
  those is modified; this spec makes spec 025 FR-012's signing statement true
  for new versions.
- Implementation tickets cite `- 026-model-rights-compliance` under
  `## Governing Spec`.
- Traverse CLI inspection and runtime enforcement are out of scope and MUST
  NOT be required to close this spec's Registry-side success criteria.
