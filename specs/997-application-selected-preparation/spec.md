# Feature Specification: Application-Selected Registry Reference Preparation

**Status**: Approved (2026-09-09, v1.0.0; v1.0.1 factual correction 2026-09-09;
v1.1.0 unresolved-projection seam 2026-09-09)
**Canonical governing ID**: `997-application-selected-preparation`
**Version**: 1.1.0
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

- **FR-002**: The derived set MUST be produced by one crate-owned
  projection, never assembled by the caller. That projection is offered in
  two forms over the same two per-component inputs
  (`components[].manifest.registry_ref` and that component's
  `permitted_targets`):
  - over an already-resolved `ApplicationBundleManifest`
    (`application_selected_references`); and
  - over an application manifest's on-disk component-manifest tree, parsed
    but with **no `registry_ref` component resolved**
    (`application_selected_references_from_manifest_path`) -- for a caller
    that must choose what to prepare *before* a resolver-backed offline cache
    exists (the load-order cycle: a `RegistryComponentResolver` reads the
    prepared cache; the cache is filled by preparing the selected set; the
    resolved-manifest form of the selected set otherwise needs that
    resolver).

  Both forms MUST return the identical set for any manifest that resolves
  fully. Deduplication is by `(namespace, id, version_range)` together with
  the component-declared target narrowing (FR-005); two components declaring
  the same reference with different narrowings are distinct selections.

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

- **FR-009** (v1.1.0): The unresolved-tree form of the FR-002 projection
  MUST apply the same component-reference uniqueness and `registry_ref`
  source-shape validation (exactly one of `contract_path` / `registry_ref`;
  a non-empty `namespace` / `id` / `version_range`) as the full manifest
  load, and MUST perform no contract retrieval, digest verification,
  dependency resolution, state-machine or connector-binding validation, or
  any `RegistryComponentResolver` call. It is a parse-and-project seam only,
  and introduces no failure code beyond `044-application-bundle-manifest`'s
  existing manifest-load taxonomy.

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
- **SC-006** (v1.1.0): For a bundle whose single `registry_ref` component
  resolves cleanly,
  `application_selected_references_from_manifest_path(path)` equals
  `application_selected_references(&load_application_bundle_manifest_with_resolver(path, Some(&r)).unwrap())`,
  and the former succeeds where a resolver-less
  `load_application_bundle_manifest(path)` fails with
  `RegistryReferenceRequiresResolution`.

## Governing Relationship

Layered on `crates/traverse-registry/` governance and `996`, per
`013-inherited-registry-governance` FR-002. Adds a constraint; supersedes no
existing registry spec. `traverse-registry` `0.20.0` is the first release
that carries `996` + this spec together: the already-published `0.19.0`
(tag `v0.19.0`, #398) predates the spec-997 batch and the
`RegistryReference` convergence. See decision-log entry 101 (which
corrects the "0.19.0 was never published" premise in entries 99/100).

**v1.1.0 amendment (2026-09-09, registry #415).** FR-002 originally named "a
single crate-owned helper over the `ApplicationBundleManifest`" as the sole
selection projection. Consumers (Traverse #1319) hit a load-order cycle:
that helper needs a resolved manifest, resolution needs a
`RegistryComponentResolver`, and the resolver reads the very offline cache
that is only filled by *preparing the selected set*. v1.1.0 keeps one
crate-owned projection but offers it in two forms -- resolved-manifest and
unresolved-manifest-tree -- that MUST agree (FR-002, FR-009, SC-006). The
change is additive: no existing signature, failure code, or the FR-001
structural guarantee moves. The repo owner selected this "amend 997"
path over a new layered spec on 2026-09-09 (registry-ops spec-path
decision). First carried by `traverse-registry` `0.21.0`.

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
