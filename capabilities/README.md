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

Six reference-app capabilities are published, across three namespaces. Each has gone
through three states: the original `1.0.0` (a 36-byte placeholder, **deprecated**),
an intermediate fixed-output fixture (also **deprecated**), and a current version with
real, input-dependent, ABI-compliant logic:

| namespace | id | current version |
|---|---|---|
| `traverse-starter` | `traverse-starter.process` | 1.1.0 |
| `traverse-starter` | `traverse-starter.validate` | 1.1.0 |
| `traverse-starter` | `traverse-starter.summarize` | 1.1.0 |
| `doc-approval` | `doc-approval.analyze` | 1.2.0 |
| `doc-approval` | `doc-approval.recommend` | 1.1.0 |
| `meeting-notes` | `meeting-notes.process` | 1.1.0 |

**Resolved (2026-07-28, first pass)**: the original `1.0.0`s all shared one identical
36-byte stub digest — concrete proof no real content had ever been built. Each `1.0.1`
used a real, distinct, source-backed WASM artifact (sourced from
`traverse-framework/traverse`'s `examples/` tree), but every one was a fixed-output
fixture — none of them actually read their input.

**Resolved (2026-07-28, second pass)**: all six capabilities now have genuinely
input-dependent logic, implemented in standalone `no_std` Rust crates under `agents/`
(source, not just artifacts, is committed in this repo). Along the way, a real
regression was caught and fixed: the *first* real-logic attempt
(`doc-approval.analyze` 1.1.0, `std` + `serde_json` + `wasm32-wasip1`) was genuinely
input-dependent but imported `wasi_snapshot_preview1::environ_get`/`environ_sizes_get`
— confirmed by inspecting its compiled import table — which fails Traverse's own
`WasmExecutor` ABI whitelist (`host_abi_v1.json`: only `fd_read`/`fd_write`/`proc_exit`
are allowed). It ran fine under a generic `wasmtime` host (how it was first verified)
but could not have executed through `traverse-cli agent execute`/`serve`/an embedder
SDK. All six capabilities are now built on a shared `no_std` shim,
[`agents/wasi-agent-runtime/`](../agents/wasi-agent-runtime) (bump allocator, hand-rolled
JSON parse/write, WASI glue) — verified via `wasm2wat` to import only the three
whitelisted WASI functions, and executed end-to-end via `wasmtime` with distinct,
genuinely computed output per capability. `doc-approval.analyze` 1.1.0 is itself now
deprecated for this reason; its replacement is 1.2.0, not 1.1.1, since 1.1.0 was never
withdrawn from the index before the fix.

Every prior version (`1.0.0` stubs, `1.0.1`/`1.1.0` fixtures-or-ABI-incompatible
releases) stays published and resolvable by exact pin — immutability preserved,
deprecation is additive via a `deprecated.json` sibling (spec 005), never an edit.
Full history: `#69` section 1.1, `#79`, `docs/decision-log.md`.

**Known, disclosed limitations of the current logic** (reference-tier heuristics, not
production-grade NLP — each capability's own contract `description` says so): the
two-consecutive-capitalized-word heuristic in `doc-approval.analyze` can false-positive
on sentence-initial phrases (e.g. "This Agreement"); the leading-capitalized-word
heuristic in `meeting-notes.process` can false-positive on sentence-initial pronouns
(e.g. "We agreed..." yields `made_by: "We"`).
