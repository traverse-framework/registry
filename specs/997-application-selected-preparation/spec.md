# Feature Specification: Application-Selected Registry Reference Preparation

**Status**: Approved (2026-09-09, v1.0.0)
**Canonical governing ID**: `997-application-selected-preparation`
**Version**: 1.0.0
**Layers on**: `996-registry-app-preparation` (the versioned single-reference
preparation contract). This spec constrains *which* references may be
prepared for offline activation; it does not restate 996's per-reference
verification pipeline or its FR-005 taxonomy.

## Why This Is A Separate Spec, Not A 996 Amendment

`996-registry-app-preparation` is a faithful, immutable mirror of
`traverse-framework/traverse` Spec 996. The manifest-scoping constraint here
is a registry-local addition; layering it as its own spec keeps 996 a clean
mirror and gives the constraint its own FRs and acceptance scenarios -- the
same pattern `021-host-load-trust-boundary-adoption` used against `traverse`
Spec 127.

**Approval note**: the repo owner approved this spec directly in the
`/brainstorm` session on 2026-09-09 (`docs/decision-log.md` entry 99).
Registered straight into `specs[]` with `status: approved` (no
`draft_specs[]` detour), the same path `021` and `024` used.

## Purpose

Ensure that only the Registry references an application's own manifest
declares are prepared into the verified cache that offline validation,
activation, and execution consume. Preparation of a reference an application
did not select is not merely discouraged -- the sanctioned entrypoint for
activation preparation cannot express it.

## Requirements

- **FR-001**: `traverse-registry` MUST expose a manifest-scoped batch
  preparation entrypoint whose *only* reference input is the set derived from
  an `ApplicationBundleManifest`'s components -- specifically every
  `components[].manifest.registry_ref`. A caller cannot supply a reference
  that is not in that derived set.

- **FR-002**: The derived set MUST be produced by a single crate-owned
  helper over the `ApplicationBundleManifest`, not assembled by the caller.
  Deduplication is by `(namespace, id, version_range)` together with the
  component-declared target narrowing (FR-005); two components declaring the
  same reference with different narrowings are distinct selections.

- **FR-003**: Preparing references for offline activation MUST go through
  this batch entrypoint. `996` FR-001's single-reference
  `prepare_application_registry_reference` remains public and is the
  primitive this batch composes, but invoking it directly *for activation
  preparation* is non-conformant. Non-activation uses (tooling, tests,
  diagnostics) are unaffected.

- **FR-004**: The batch MUST run each selected reference through the full
  `996` FR-002..FR-007 pipeline, in `996`'s order, and MUST stop at the
  first reference that fails (fail-fast). Its result MUST report both the
  `VerifiedRegistryPreparation` evidence for every reference prepared before
  the stop and the `(reference, failure)` pair for the one that failed;
  `failed` is `Some` iff the batch is incomplete. Cache entries committed
  for earlier references remain valid (`996` FR-007: digest-keyed,
  immutable), so a re-run resumes without re-fetching them.

- **FR-005**: `policy` (the host network/host/permission decision, `996`
  FR-003) and `supported_abi` are one value each for the whole batch. The
  requested target is one value for the whole batch; per reference, the
  batch MUST additionally verify it against that component's
  manifest-declared `permitted_targets` -- a manifest may only tighten the
  index record's permitted targets, never widen them. An empty
  component `permitted_targets` imposes no narrowing (no invented fallback,
  matching `996` FR-004).

- **FR-006**: A manifest with no `registry_ref` components MUST prepare
  trivially -- an empty `prepared` set and no failure -- never an error.

- **FR-007**: This spec is additive. It introduces no new failure code
  (`996` FR-005's twelve codes are the complete taxonomy), changes no
  published capability record, and grants no runtime network access. All
  `996` redaction guarantees (FR-008) and `serde`-consumability (FR-009)
  carry through unchanged: no batch result value carries a URL, endpoint,
  credential, header, host path, or raw bytes.

- **FR-008**: Where this spec and `traverse` Spec 996 diverge on
  registry-owned behavior, `traverse` Spec 996 and its registry mirror are
  authoritative for the per-reference pipeline; this spec governs only the
  selection scope and the batch result shape, and is amended if that layer
  is later moved into `traverse` Spec 996 itself.

## Success Criteria

- **SC-001**: The batch entrypoint's reference input has no public
  constructor that admits a reference absent from the manifest-derived set
  (enforced structurally, verified by the type signature and a test).
- **SC-002**: `application_selected_references` over a manifest with three
  `registry_ref` components (one duplicated) yields exactly the distinct
  declared references.
- **SC-003**: A batch of N references where the k-th fails
  (`k < N`) returns `prepared.len() == k - 1`, `failed ==
  Some((k-th reference, <its 996 code>))`, and leaves the k-1 committed
  cache entries intact; a re-run with the k-th reference's retrieval fixed
  completes.
- **SC-004**: A selected reference whose requested target is permitted by
  the index record but excluded by the component's manifest
  `permitted_targets` fails with `996`'s `registry_target_incompatible`.
- **SC-005**: A manifest with only local (no `registry_ref`) components
  returns `{ prepared: [], failed: None }`.

## Governing Relationship

Layered on `crates/traverse-registry/` governance and `996`, per
`013-inherited-registry-governance` FR-002. Adds a constraint; supersedes no
existing registry spec. `traverse-registry` `0.19.0` carries both `996` and
this spec (the version was not yet published when `996` landed; see
decision-log entries 97 and 99).

## Out of Scope

- Resolving a single per-component execution target from the manifest's
  untyped app-level `placement_policy` -- a larger design task; this spec
  takes one batch-wide requested target and only narrows it per component.
- Any change to `996`'s per-reference pipeline, its FR-005 taxonomy, or the
  `SignatureVerifier` / retrieval / cache adapter model.
- Workflows and connector bindings in an application manifest -- this spec
  is about capability `registry_ref` components only.
- Propagating per-version signature evidence through `registry sync` (a
  `996` follow-up noted in decision-log entry 97).
