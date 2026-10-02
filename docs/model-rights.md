# Model rights: publisher and consumer obligations

Governed by [`specs/026-model-rights-compliance`](../specs/026-model-rights-compliance/spec.md)
(decision-log entry 127). A worked example lives in
[`examples/model-rights/`](../examples/model-rights/README.md).

The model rights record is **declarative**: the publisher states it, and the
Registry signs the published contract. It is **not** a legal certification.
Capability-level `licensing` (spec 025) never grants or denies rights for the
models a capability embeds. Those rights live only on each `ai.models` entry.

## Publisher obligations

Every newly added contract with `ai.model_backed: true` must give each
object-shaped `ai.models` entry the following, on top of spec 001 FR-017's
`id`, `spdx_expression` and `attribution_required`:

| Field | Rule | CI error code on failure |
|---|---|---|
| `commercial_use`, `redistribution`, `derivatives` | `allowed` \| `forbidden` \| `conditional`. Research them at the pinned revision. `unknown` is rejected. | `contract.model_rights_unknown`, `contract.invalid_model_rights` |
| — | `redistribution: forbidden` is rejected because hosting the WASM that embeds the weights *is* redistribution. `derivatives: forbidden` cannot be combined with a `derivation` object, and a non-redistributable SPDX marker cannot be combined with `redistribution: allowed`. | `contract.model_rights_contradiction` |
| `revision` or `source_url` | Must pin a full upstream commit id. Branches and tags are rejected. | `contract.model_rights_mutable_revision` |
| `license_files[]` | Required. Each entry is `{url, sha256}` and must be an asset on this repo's `artifacts/<tag>` Release. CI downloads it and verifies the digest. | `contract.invalid_model_rights_evidence_url`, `contract.model_rights_evidence_unreachable`, `contract.model_rights_evidence_digest_mismatch` |
| `notice_files[]` | Same shape. Required when `attribution_required` is `true`. | `contract.model_rights_missing_notice` |
| `verification` | `{status: "maintainer-declared", evidence_url?}`. Other statuses are reserved. A `LicenseRef-*` expression also needs `evidence_url`. | `contract.invalid_model_rights_verification`, `contract.model_rights_licenseref_needs_evidence` |
| `derivation` | Required key. Use `null` when the weights ship byte-for-byte from upstream. Otherwise give `{transformations: [quantize\|format-convert\|prune\|distill\|fine-tune\|other], description, tool_url?, converted_sha256}`. `converted_sha256` must appear in the crate's `model-weights.json`. | `contract.model_rights_missing_derivation`, `contract.invalid_model_derivation`, `contract.model_derivation_digest_unlisted` |
| `data_obligations[]` | Optional. Each entry is `{dataset, kind: training\|labels\|eval, obligation, source_url, spdx_expression?}`. | `contract.invalid_model_data_obligations` |
| `rights_change` | `{reason, evidence_url}`. Required whenever SPDX, attribution, or a rights enum differs from any other published contract citing the same model and pin. That covers both earlier versions of your capability and other capabilities. | `contract.model_rights_drift`, `contract.invalid_model_rights_change` |

Upload LICENSE, NOTICE and weight blobs to the same `artifacts/<id>-<version>`
Release as the WASM artifact before you open the PR. Already-published
versions are never re-judged; to bring one into compliance, publish a new
MINOR version.

## Admission fixture corpus

[`scripts/ci/fixtures/ai_admission_corpus.json`](../scripts/ci/fixtures/ai_admission_corpus.json)
lists accept/reject `ai` objects together with the exact error codes that
`capability_validation.py` produces for them. It covers both spec 001
FR-017 and the rights record above. The test
`scripts/ci/tests/test_ai_admission_corpus.py` runs every fixture through
the validator, so any change in CI's verdict on a fixture fails the build.

`traverse-cli capability publish` vendors a pinned copy and must agree with
every fixture tagged `contract_decidable` and evaluated as `newly_added`.
That is how a publisher sees these errors before a Registry PR exists.
Fixtures tagged `ci_only` cover the checks only CI can run: evidence digest
fetches and the `model-weights.json` cross-check.

**When you change an `ai` rule, add or update fixtures in the same PR** and
bump `corpus_version`. Use a minor bump for a new fixture, and a major bump
when an existing fixture's verdict changes. A rule change without a fixture
change is incomplete. The Traverse pin is then bumped deliberately in a
Traverse PR.

## Signing and revocation

- **Contract signature.** Every new `signature.json` covers the exact committed
  `contract.json` bytes (`contract_sha256` + `contract_signature_hex`). That
  authenticates the whole rights record above. See
  [`artifact-signing.md`](artifact-signing.md#contract-signature-spec-026-fr-012-registry621).
- **Revocation.** To withdraw a version, for example after an upstream takedown
  or relicensing, add a sibling `revoked.json` by PR:

  ```json
  {
    "reason": "Upstream relicensed the weights under non-redistributable terms.",
    "evidence_url": "https://huggingface.co/<org>/<model>/discussions/<n>",
    "revoked_at": "2026-10-01T00:00:00Z"
  }
  ```

  CI validates its shape (`revocation.invalid`, `revocation.orphaned`) and
  treats it as immutable once merged (`capabilities.revocation_modified`). The
  contract and artifact are never touched. The index keeps the entry with
  `status: "revoked"`, a `revocation` record, and its full rights record.

## Consumer obligations

- **Deny by default.** Treat `conditional` as "read the license", never as
  permission. New model references cannot carry `unknown`. Legacy references
  (string-shaped, or object-shaped before spec 026) carry no rights enums;
  treat them as unknown.
- **Ship the NOTICE.** When `attribution_required` is `true`, redistribute the
  `notice_files` content alongside anything that embeds the model.
- **Honor `status`.** Index entries carry `status: active | deprecated |
  revoked`. A revoked version is never active. Exact pins still resolve, but
  the runtime should refuse to run the version.
- **Use `model_usage` only as a filter.** The index's derived `usage_class`
  (`unrestricted` | `evaluation-only` | `conditional`) is a projection of the
  rights enums. The full record in `ai.models` stays authoritative.
- **No `data_obligations` means "not stated".** Absence never means "none".
