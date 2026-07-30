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
| `traverse-starter` | `traverse-starter.process` | 1.2.1 |
| `traverse-starter` | `traverse-starter.validate` | 1.2.1 |
| `traverse-starter` | `traverse-starter.summarize` | 1.2.1 |
| `doc-approval` | `doc-approval.analyze` | 1.3.1 |
| `doc-approval` | `doc-approval.recommend` | 1.2.1 |
| `meeting-notes` | `meeting-notes.process` | 1.3.1 |

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

**`use_cases` backfilled (2026-07-29, #107, additive minor bump each)**: all six now
carry a `use_cases` array (spec 001 FR-011) with concrete, `wasmtime`-verified
input/output example pairs (schema-conformant, not hand-waved) -- authored fresh for
these six, since no prior use-case documentation existed for them despite an earlier
(inaccurate) README claim to that effect.

**`scenario` rewritten as a full user story (2026-07-30, #139, decision-log entry 46,
patch bump each)**: every `use_cases[].scenario` was a plain declarative sentence
(e.g. "A document with a recognized type... reaches high confidence.") rather than the
"As a `<persona>`, I want to `<action>`, so that `<benefit>`." format FR-011 was
amended to require. Rewritten with real personas grounded in how each capability is
actually used (an accounts-payable clerk, an approvals manager, a meeting organizer) --
`input_example`/`output_example`/`happy` are untouched, only `scenario` text changed,
which `capability_validation.py`'s own `classify_change()` confirms is a patch-class
change, not minor.

**Kit workflows published (2026-07-29, #124, spec 001 FR-013)**: `traverse-starter`
(`validate` -> `process` -> `summarize`) and `doc-approval` (`analyze` -> `recommend`)
are now published as first-class, versioned workflow records under
[`workflows/traverse-starter/traverse-starter.process-note/1.0.0/`](../workflows/traverse-starter/traverse-starter.process-note/1.0.0)
and
[`workflows/doc-approval/doc-approval.review-document/1.0.0/`](../workflows/doc-approval/doc-approval.review-document/1.0.0),
included in the public index's new `workflows[]` array (same build/immutability rules
as `capabilities[]`). `meeting-notes.process` has no natural multi-step pipeline, so it
instead got a minor-version republish (1.2.0 -> 1.3.0) adding a boolean
`"entrypoint": true` marker directly on its own contract, per FR-013's explicit
single-capability-entrypoint carve-out -- no workflow wrapper forced around one
capability. See [`workflows/README.md`](../workflows/README.md) for the full layout.

**Runnable example requests published (2026-07-29, #125)**: each kit entrypoint (the
two workflows above, plus `meeting-notes.process`) now has a standalone
`example-request.json` sibling -- a real `request`/`expected_response` pair, verified
against the actual compiled WASM binaries (chained through every workflow node via
`wasmtime run`, not hand-traced), so a consumer can `curl`/pipe a request without
hand-composing one from `use_cases`.

**"kit-llm" sync profile documented (2026-07-29, #126)**: see
[`docs/kit-llm-sync-profile.md`](../docs/kit-llm-sync-profile.md) for how an MCP host
or any LLM-facing consumer runs `registry sync` and filters the index's
`capabilities[]`/`workflows[]` down to just this curated kit content (`core`,
`traverse-starter`, `doc-approval`, `meeting-notes` -- explicitly not the
`validation`/`formatting` utility-tier namespaces below). No new registry-side
artifact or release process -- a documented convention over the existing mechanism.

## Utility-tier capabilities (added 2026-07-29)

Five general-purpose validation/formatting capabilities, published to dogfood the
`traverse-capability-author` Claude Skill end-to-end (see `docs/decision-log.md`
entry 39 for the full account, including a real production incident this work
surfaced and fixed):

| namespace | id | current version |
|---|---|---|
| `validation` | `validation.validate-email` | 1.2.1 |
| `validation` | `validation.normalize-phone-number` | 1.2.1 |
| `validation` | `validation.score-password-strength` | 1.1.1 |
| `validation` | `validation.validate-luhn` | 1.1.1 |
| `formatting` | `formatting.format-currency` | 1.1.1 |

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

Happy/unhappy-path behavior for all five is exercised directly in each crate's own
`#[cfg(test)] mod tests` under `capability-src/` (no separate `SPEC.md` file ever
existed for these, despite an earlier version of this README claiming one did -- a
stale claim corrected in the same #107 pass that backfilled `use_cases`, below), and,
per this registry's own disclosure convention, an honestly-documented known limitation
found only by implementing and testing, not assumed up front: `normalize-phone-number`
does not strip domestic trunk prefixes (e.g. UK `020 7946 0958`);
`score-password-strength`'s scoring model was corrected during implementation to match
verified, self-consistent behavior rather than an earlier draft's imprecise worked
examples.

**`use_cases` backfilled (2026-07-29, #107, additive minor bump each)**: all five now
carry a `use_cases` array (spec 001 FR-011), restructuring the same test-verified
happy/unhappy examples above into the schema's `{scenario, input_example,
output_example, happy}` shape -- each pair independently re-verified against the real
compiled `wasmtime` output as part of this backfill, not just copied from the test
source.

**`scenario` rewritten as a full user story (2026-07-30, #139, decision-log entry 46,
patch bump each)**: same rewrite as the six reference-app capabilities above, with
personas grounded in these five's own real usage (a signup/checkout/payment form
developer) -- `input_example`/`output_example`/`happy` untouched, only `scenario` text
changed.

Source for all five lives under `capability-src/` (`validate-email/`,
`normalize-phone-number/`, `score-password-strength/`, `validate-luhn/`,
`format-currency/`), built on the same shared `capability-src/wasi-capability-runtime/`
shim as the six reference capabilities above.
