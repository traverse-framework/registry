# Capabilities Directory

This directory is the actual registry content: one file tree per published capability version.

## Layout

```text
capabilities/<namespace>/<id>/<version>/contract.json
```

- `<namespace>` — reserved for future third-party publishers; every published capability today uses an app-scoped namespace matching its reference app (`traverse-starter`, `doc-approval`, `meeting-notes` — see `specs/008-reference-capability-publication/spec.md`), not a shared default. A `core`-owned unscoped default remains reserved for future use (see `specs/001-registry-foundation/spec.md`, FR-008) but nothing is published there yet.
- `<id>` — the capability identity within its namespace.
- `<version>` — an exact semver version. Each version is its own immutable directory — never edit an existing version's `contract.json` after merge. To fix a bad publish, see the deprecation/yank process in `specs/001-registry-foundation/spec.md` (User Story 4).

Artifact binaries (WASM, etc.) are **not** committed here — they're referenced by digest + GitHub Release URL from within `contract.json` (see `specs/001-registry-foundation/spec.md`, FR-007).

## Current content (as of 2026-07-28)

Six reference-app capabilities are published, across three namespaces:

| namespace | id | version |
|---|---|---|
| `traverse-starter` | `traverse-starter.process` | 1.0.0 |
| `traverse-starter` | `traverse-starter.validate` | 1.0.0 |
| `traverse-starter` | `traverse-starter.summarize` | 1.0.0 |
| `doc-approval` | `doc-approval.analyze` | 1.0.0 |
| `doc-approval` | `doc-approval.recommend` | 1.0.0 |
| `meeting-notes` | `meeting-notes.process` | 1.0.0 |

**Known gap, not yet resolved**: every artifact above resolves to the identical digest
`sha256:5647c39a1d25d8728350f9619025292a62e78a602068a2ad9b6f075751c93d99` — a 36-byte
placeholder, not six distinct real agent binaries. `contract.json`'s own digest field is
correct (it matches what was actually released), so digest verification passes as designed
— but a consumer resolving any of these six IDs today gets a stub, not production logic.
Tracked in [#69](https://github.com/traverse-framework/registry/issues/69) section 1.1
as the top-priority gap; each of these versions is immutable once published (spec 007),
so replacing the stub requires a new semver version plus, per spec 005, yanking or
otherwise marking the `1.0.0` stub excluded from range resolution once a real successor
exists — not an edit to the existing release.
