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

Six reference-app capabilities are published, across three namespaces. Each has two
versions: the original `1.0.0` (a 36-byte placeholder, **deprecated**, kept only for
immutability/provenance — never resolve it deliberately) and `1.0.1` (current):

| namespace | id | current version |
|---|---|---|
| `traverse-starter` | `traverse-starter.process` | 1.0.1 |
| `traverse-starter` | `traverse-starter.validate` | 1.0.1 |
| `traverse-starter` | `traverse-starter.summarize` | 1.0.1 |
| `doc-approval` | `doc-approval.analyze` | 1.0.1 |
| `doc-approval` | `doc-approval.recommend` | 1.0.1 |
| `meeting-notes` | `meeting-notes.process` | 1.0.1 |

**Resolved (2026-07-28)**: the original `1.0.0`s all shared one identical 36-byte stub
digest — concrete proof no real content had ever been built. Each `1.0.1` uses a real,
distinct, source-backed WASM artifact (sourced from `traverse-framework/traverse`'s
`examples/` tree) and each `1.0.0` is now marked `deprecated` (spec 005 — additive record,
the original `contract.json` files are untouched, immutability preserved).

**Known gap, still open**: the `1.0.1` artifacts are genuine, distinct WASI binaries, but
each is a fixed-output fixture — none of them actually read their input; every call to a
given capability returns the same hardcoded response regardless of what's passed in. Each
`1.0.1` contract's `description` says this plainly rather than implying real per-input
analysis. Writing real, input-dependent deterministic logic is tracked separately as
[#79](https://github.com/traverse-framework/registry/issues/79) — not blocking, not
bundled into the artifact-hosting fix above. Full history: `#69` section 1.1,
`docs/decision-log.md` entries 34-35.
