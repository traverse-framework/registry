# Capabilities Directory

This directory is the actual registry content: one file tree per published capability version.

## Layout

```text
capabilities/<namespace>/<id>/<version>/contract.json
```

- `<namespace>` — the publisher-scoped identity prefix. It must match the
  path segment and is distinct from a bundle's public/private resolution
  scope; see `specs/006-public-scope-and-identity/spec.md`.
- `<id>` — the capability identity within its namespace.
- `<version>` — an exact semver version. Each version is its own immutable directory — never edit an existing version's `contract.json` after merge. To fix a bad publish, see the deprecation/yank process in `specs/001-registry-foundation/spec.md` (User Story 4).

Artifact binaries (WASM, etc.) are **not** committed here — they're referenced by digest + GitHub Release URL from within `contract.json` (see `specs/007-artifact-hosting/spec.md`).

## Current published namespaces

The current reference seed records are published under these namespaces:

- `traverse-starter`
- `doc-approval`
- `meeting-notes`

Their public index is built and published as a versioned GitHub Release. A
consumer syncs that index before resolving a `registry_ref`; it does not query
this repository at execution time. See `specs/003-index-release-pipeline/spec.md`
and `specs/008-reference-capability-publication/spec.md` for the publication
and reference-app policies.

## Reference seed status

The records currently in this directory are reference seeds. Their published
artifacts are placeholders and must not be treated as product-ready binaries.
Replacement artifacts require a new immutable version, followed by an additive
deprecation record for any superseded version; existing `contract.json` files
are never edited or deleted. See `specs/007-artifact-hosting/spec.md` and
`specs/005-yank-deprecation/spec.md`.
