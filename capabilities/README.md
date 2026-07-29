# Capabilities Directory

This directory is the actual registry content: one file tree per published capability version.

## Layout

```text
capabilities/<namespace>/<id>/<version>/contract.json
```

- `<namespace>` — reserved for future third-party publishers; identifies *who* published a capability, never *what topic* it's about (that's `use_cases`/`summary`/`description`'s job, not namespace's — see `specs/001-registry-foundation/spec.md`, FR-008/FR-012). Two coexisting conventions for core-team publishes: app-scoped, matching the reference app it belongs to (`traverse-starter`, `doc-approval`, `meeting-notes` — see `specs/008-reference-capability-publication/spec.md`), when a real reference app exists; `core`, for any general-purpose capability with no natural reference app, going forward. **Historical exception**: the 5 `validation`/`formatting` utility capabilities (below) were published before this policy existed and used ad hoc topic-based namespaces instead — left exactly as published, permanently (namespace is part of a capability's public identity; republishing under `core` would create a confusing permanent duplicate, not a fix). Namespace claiming/identity verification for real third-party publishers remains out of scope (spec 001's own stated assumption).
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
input-dependent logic, implemented in standalone `no_std` Rust crates (source, not
just artifacts, is committed in this repo). Along the way, a real
regression was caught and fixed: the *first* real-logic attempt
(`doc-approval.analyze` 1.1.0, `std` + `serde_json` + `wasm32-wasip1`) was genuinely
input-dependent but imported `wasi_snapshot_preview1::environ_get`/`environ_sizes_get`
— confirmed by inspecting its compiled import table — which fails Traverse's own
`WasmExecutor` ABI whitelist (`host_abi_v1.json`: only `fd_read`/`fd_write`/`proc_exit`
are allowed). It ran fine under a generic `wasmtime` host (how it was first verified)
but could not have executed through `traverse-cli capability execute`/`serve`/an embedder
SDK. All six capabilities are now built on a shared `no_std` shim,
[`capability-src/wasi-capability-runtime/`](../capability-src/wasi-capability-runtime)
(bump allocator, hand-rolled
JSON parse/write, WASI glue) — verified via `wasm2wat` to import only the three
whitelisted WASI functions, and executed end-to-end via `wasmtime` with distinct,
genuinely computed output per capability. `doc-approval.analyze` 1.1.0 is itself now
deprecated for this reason; its replacement is 1.2.0, not 1.1.1, since 1.1.0 was never
withdrawn from the index before the fix.

Every prior version (`1.0.0` stubs, `1.0.1`/`1.1.0` fixtures-or-ABI-incompatible
releases) stays published and resolvable by exact pin — immutability preserved,
deprecation is additive via a `deprecated.json` sibling (spec 005), never an edit.
Full history: `#69` section 1.1, `#79`, `docs/decision-log.md`.

**Naming correction (2026-07-29, decision-log entry 41)**: this repo's source
directory for capability implementations was renamed from `agents/` to
`capability-src/`, and the shared runtime shim from `wasi-agent-runtime` to
`wasi-capability-runtime` (`run_agent()` → `run_capability()`). "Agent" is now a
reserved term for a future capability whose implementation genuinely involves
AI/model-backed reasoning — none of the 11 capabilities currently published here do;
they are all pure, deterministic business logic. **Six already-published, immutable
contracts, plus one deprecation record, predate this rename and have the old
`agents/...` path (or `traverse-cli agent execute`/`serve`) written directly into
their text** (`doc-approval.analyze` 1.2.0's `contract.json`, `doc-approval.analyze`
1.1.0's `deprecated.json`, `doc-approval.recommend` 1.1.0, `traverse-starter.process`/
`validate`/`summarize` 1.1.0, `meeting-notes.process` 1.1.0) — per this repo's
immutability rule, none of that text can ever be edited. Those
descriptions are historically accurate to when they were written, not to the current
layout: current source for every capability lives under `capability-src/`, not `agents/`.

**Known, disclosed limitations of the current logic** (reference-tier heuristics, not
production-grade NLP — each capability's own contract `description` says so): the
two-consecutive-capitalized-word heuristic in `doc-approval.analyze` can false-positive
on sentence-initial phrases (e.g. "This Agreement"); the leading-capitalized-word
heuristic in `meeting-notes.process` can false-positive on sentence-initial pronouns
(e.g. "We agreed..." yields `made_by: "We"`).

## Utility-tier capabilities (added 2026-07-29)

Five general-purpose validation/formatting capabilities, published to dogfood the
`traverse-capability-author` Claude Skill end-to-end (see `docs/decision-log.md`
entry 39 for the full account, including a real production incident this work
surfaced and fixed):

| namespace | id | current version |
|---|---|---|
| `validation` | `validation.validate-email` | 1.1.0 |
| `validation` | `validation.normalize-phone-number` | 1.1.0 |
| `validation` | `validation.score-password-strength` | 1.0.0 |
| `validation` | `validation.validate-luhn` | 1.0.0 |
| `formatting` | `formatting.format-currency` | 1.0.0 |

**Labeled utility-tier, not "business capabilities"**: the AI-advisory review
(`.agents/skills/capability-review/`) flagged a genuine boundary question when the
first of these was published — email/phone/password/Luhn/currency logic reads closer
to a reusable utility function than "one meaningful business action" (this repo's own
duplicate/boundary rubric). **Resolved (2026-07-29, `docs/decision-log.md` entry 42,
closes #101)**: utility-tier is a legitimate, permanent capability class — these five
are not a pilot awaiting further validation, they're staying. Deliberately **not**
given a formal `tier` schema field, though: real discovery/selection (an LLM/MCP
runtime matching a workflow step, or a human developer doing the same) should run on
the `use_cases` field (decision-log entry 40), not a business/utility label no actual
consumer's selection logic would query on. The `capability-review` skill's rubric
(`.agents/skills/capability-review/SKILL.md` and its canonical wording in
`scripts/ci/ai_advisory_review.py`) has since been refreshed to explicitly account
for both classes (#114).

Each has a companion `SPEC.md` (use cases, happy/unhappy paths, NFRs, configuration —
kept alongside the source under `capability-src/`, not in `capabilities/`, since the
contract schema itself has no field for that level of detail) and, per this registry's
own disclosure convention, an honestly-documented known limitation found only by
implementing and testing, not assumed up front: `normalize-phone-number` does not
strip domestic trunk prefixes (e.g. UK `020 7946 0958`); `score-password-strength`'s
scoring model was corrected during implementation to match verified, self-consistent
behavior rather than an earlier draft's imprecise worked examples.

Source for all five lives under `capability-src/` (`validate-email/`,
`normalize-phone-number/`, `score-password-strength/`, `validate-luhn/`,
`format-currency/`), built on the same shared `capability-src/wasi-capability-runtime/`
shim as the six reference capabilities above.
