# Feature Specification: Application Registry Reference Preparation

**Status**: Approved (2026-09-08)
**Canonical governing ID**: `996-registry-app-preparation`
**Version**: 0.1.0
**Cross-repository source**: `traverse-framework/traverse` Spec 996, approved in traverse#1284.

## Purpose

Define the versioned Registry public contract that lets a host explicitly
prepare an application's `registry_ref` from a validated synced index. The
contract supplies exact selection, policy-aware retrieval adapters, verified
immutable evidence, and stable redacted outcomes. Traverse remains an offline
consumer: validation, activation, and execution do not receive Registry
network access through this contract.

## Requirements

- **FR-001**: The Registry crate MUST expose a versioned preparation request
  that accepts a validated synced-index snapshot, Registry reference,
  host-owned network/host/permission policy decision, requested target and
  placement, and host-owned retrieval and cache adapters.
- **FR-002**: Exact version ranges MUST select only their identical active
  version. If it is unavailable, the result MUST be
  `registry_version_range_unsatisfied`; no alternate version may be selected.
- **FR-003**: Policy evaluation MUST occur before either contract or artifact
  retrieval. A denial MUST return `registry_policy_denied` and invoke no
  retrieval adapter.
- **FR-004**: Before a result is available for offline use, preparation MUST
  verify contract identity and digest, artifact digest, signature evidence,
  lifecycle, ABI, permitted target, placement, and declared constraints.
- **FR-005**: The public first-failure taxonomy MUST include
  `registry_index_selection_failed`, `registry_version_range_unsatisfied`,
  `registry_lifecycle_rejected`, `registry_policy_denied`,
  `registry_contract_unreachable`, `registry_contract_digest_mismatch`,
  `registry_artifact_unreachable`, `registry_artifact_digest_mismatch`,
  `registry_signature_unverified`, `registry_abi_incompatible`,
  `registry_target_incompatible`, and `registry_cache_commit_failed`.
- **FR-006**: A successful result MUST retain immutable identity, requested
  range, selected version, contract digest, artifact digest, trust lifecycle,
  ABI, target, placement, constraints, and non-secret resolver evidence.
- **FR-007**: The cache-writer contract MUST reject bytes that differ from an
  existing entry with the same digest key, preserving the existing entry.
- **FR-008**: Public preparation outcomes and evidence MUST NOT contain URLs,
  endpoints, credentials, authorization headers, host-private paths, or raw
  contract/artifact bytes.
- **FR-009**: The API MUST be consumable by Traverse without CLI-specific
  reinterpretation of errors or evidence.

## Acceptance Scenarios

1. Given active signed `1.0.1`, a permitted host policy, and `=1.0.1`,
   preparation returns immutable evidence for `1.0.1` only.
2. Given only `1.0.0`, preparation of `=1.0.1` returns
   `registry_version_range_unsatisfied` and selects no fallback.
3. Given a denied policy, preparation returns `registry_policy_denied` before
   a retrieval adapter is called.
4. Given an invalid signature, digest, ABI, lifecycle, or target, preparation
   returns the matching stable code and writes no cache entry.
5. Given a cache key already holding different bytes, preparation returns
   `registry_cache_commit_failed` and the original entry remains unchanged.

## Compatibility and Non-goals

This is additive to existing Registry index and publication identity semantics.
It does not change published capability records, grant runtime network access,
persist host-private configuration, rewrite application manifests, or implement
Traverse CLI behavior. API additions follow semver compatibility policy.
