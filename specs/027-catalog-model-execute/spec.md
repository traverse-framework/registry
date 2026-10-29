# Feature Specification: Catalog Governed `model.execute`

**Feature Branch**: `codex/issue-633-model-execute`
**Created**: 2026-10-15
**Status**: Draft
**Canonical governing ID**: `027-catalog-model-execute`
**Version**: 0.1.0
**Extends**: `001-registry-foundation`, `007-artifact-hosting`, `026-model-rights-compliance`;
aligned with Traverse Spec `138-governed-exact-model-execution` and connector
`traverse.model-runtime` (pinned at publish time).
**Input**: registry#633 — catalog consumers need a discoverable capability that
dispatches signed exact-ref model packages through Spec 138, without forking
connector schemas that live in `traverse`.

## Purpose

Publish `model.execute@1.0.0` under `capabilities/model/model.execute/` so MCP
Mode A, web `/discover`, and agents can find and invoke governed model execution.
The capability is a mediated connector shim: it validates catalog-facing inputs
(`model_ref`, `model_usage`, `trust_root`, staging refs), calls
`traverse.model-runtime` / `model.execute`, and returns host `model_evidence` or
a fail-closed Spec 138 `reason`.

## Functional requirements

- **FR-001**: The catalog MUST publish `model.execute` with
  `connector_requirements` pinning `traverse.model-runtime` at a concrete
  contract version present on Traverse main at publish time.
- **FR-002**: Inputs MUST require an exact-ref `model_ref` (`model_id`,
  `version`, `digest`), `model_usage` ∈ {`commercial`,`non_commercial`}, and a
  non-empty `trust_root.allowed_signers`.
- **FR-003**: Missing `model_usage` MUST surface `usage_undeclared`; empty
  trust root MUST surface `key_untrusted` before connector dispatch.
- **FR-004**: Host fail-closed reasons
  (`digest_mismatch`, `key_untrusted`, `signature_invalid`, `rights_mismatch`,
  `rights_policy_denied`, `usage_undeclared`, `package_revoked`,
  `model_unavailable`) MUST pass through unchanged on the capability output.
- **FR-005**: Happy-path output MUST include `model_evidence` when the host
  returns it. From `model.execute@1.1.0` (registry#641), `model_evidence` MUST
  be the normative flat guest-visible shape `{license_id, commercial_use,
  status, effective_usage}` with optional `redistribution` and `derivation`
  (no `source_url` / nested `rights` / Spec 104 host-private strings).
  Forwarding the full host `ModelRightsRecord` is non-conformant.
- **FR-006**: Connector schemas MUST NOT be forked into this repo; contracts
  reference Traverse connector + schema IDs only.
- **FR-007**: At least one signed exact-ref package usable through the
  capability (e.g. Traverse `traverse.digits-onnx@1.0.0`) MUST be documented in
  the contract description / use cases. Live Hugging Face fetches remain out of
  scope (decision-log 124 / #575).

## Out of scope

- Rewriting baked-in agent capabilities onto `model.execute`.
- Production model-signing key custody (traverse#1567).
- Approving this draft (owner moves it into `specs[]`).

## Success criteria

- Contract + WASM + `capability-src/model-execute` merged with registry CI green.
- Conformance vectors for happy path and listed fail-closed reasons covered by
  capability unit tests (host Browser/Node/native parity remains Spec 138's
  shared suite).
