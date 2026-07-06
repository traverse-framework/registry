# Feature Specification: Reference App Capability Publication

**Feature Branch**: `008-reference-capability-publication`
**Created**: 2026-07-06
**Status**: Approved
**Version**: 1.0.0
**Input**: Registry issues #21–#22, Traverse **056-capability-publish**, App-References reference apps.

## Purpose

Govern how **reference-app capabilities** (traverse-starter, doc-approval, meeting-notes) are published to `traverse-framework/registry` as public seed artifacts consumable via `registry_ref` per Traverse **054**.

This spec **does not duplicate** **002-capability-validation** or **007-artifact-hosting**; it defines the **reference-app publication checklist** and namespace policy.

## Namespace Policy

| Namespace | Owner | Scope | Reference apps |
|-----------|-------|-------|----------------|
| `traverse-starter` | core | public | traverse-starter.* |
| `doc-approval` | core | public | doc-approval.* |
| `meeting-notes` | core | public | meeting-notes.* |

Reference capabilities MUST use `scope: public` and MUST NOT publish private-scope bundles to the public registry.

## Publication Checklist (per capability)

- [ ] Contract at `capabilities/<namespace>/<id>/<version>/contract.json`
- [ ] Artifact hosted per **007-artifact-hosting** with SHA-256 digest
- [ ] CI validation per **002-capability-validation** passes
- [ ] Semver bump matches diff class
- [ ] `traverse-cli capability publish --dry-run` succeeds (Traverse **056**)
- [ ] Registry PR merged; index release job green (**003**)

## Seed Set (v1)

| Capability ID | Registry issue | Traverse implementation |
|---------------|----------------|-------------------------|
| `traverse-starter.process` | #21 | Done in Traverse #499 |
| `traverse-starter.validate` | #25 | Traverse #554 |
| `traverse-starter.summarize` | #26 | Traverse #554 |
| `doc-approval.analyze` | #27 | Traverse #556 |
| `doc-approval.extract` | #28 | Traverse #538 |
| `doc-approval.recommend` | #29 | Traverse #555 |
| `meeting-notes.process` | #30 | Traverse #532 |

## Functional Requirements

- **FR-001**: Reference capabilities MUST publish under namespace matching app id prefix.
- **FR-002**: Registry CI MUST reject reference capabilities missing owner `core` without explicit approval.
- **FR-003**: Each seed capability MUST include determinism test evidence in Traverse repo before publish PR.
- **FR-004**: Published digests MUST match Traverse example WASM artifacts linked from publication PR.
- **FR-005**: App-References manifests MAY use `registry_ref` only after corresponding registry path exists on `main`.

## Relationship to Traverse Specs

| Traverse spec | Role |
|---------------|------|
| **054-public-scope-registry-ref** | Resolution of `registry_ref` in app manifests |
| **056-capability-publish** | CLI automation for opening registry PRs |
| **058-workflow-pipeline-execution** | Pipeline steps may reference registry-published capability ids |

## Out of Scope

- Private app capabilities
- Non-reference third-party namespaces (governed by **006-public-scope-and-identity**)

## Downstream

- Unblocks reference-apps Phase 2 `registry_ref` adoption (**#97**)
- Unblocks registry **#21** seed publication
