# Feature Specification: Sanitized Display Metadata in the Public Sync Index

**Feature Branch**: `claude/issue-312-public-metadata-projection`
**Created**: 2026-08-25
**Status**: Draft
**Input**: Registry ticket #312 — supply the verified, public-only input for
`traverse-framework/traverse` spec `116-verified-public-contract-metadata-cache`
(Approved 2026-08-24) FR-003.

## Purpose

Spec `116` (traverse-side) defines a local, offline metadata cache for MCP
search (`traverse#876`) and browser entrypoint resolution (`traverse#1105`).
Its FR-003 requires the cache's public metadata projection to carry
"identity/version, display metadata, description, and declared
`use_cases[].scenario` text" while excluding raw contracts, use-case
input/output examples, private fields, and secrets.

`index.json` (`specs/003-index-release-pipeline`, extended by
`specs/009-contract-metadata-in-index`) already carries verified identity,
artifact digest, and a `contract_digest`/`contract_url` pointer to the full
`contract.json`. A cache-preparation step could technically satisfy spec
`116` today by following that pointer and fetching every capability's full
contract individually, then discarding everything but the sanitized fields
client-side — but that is an O(n) fetch per capability just to build one
search index, wasteful the moment this registry's catalog grows past a
handful of entries, and it means every consumer of this data reimplements
the same sanitization boundary instead of it being defined once, here. This
spec adds the sanitized fields directly to `index.json` so a single fetch
already carries what spec `116`'s cache needs.

This is additive to `specs/003-index-release-pipeline` and
`specs/009-contract-metadata-in-index`, both of which stay unmodified
(immutable) — this spec extends `scripts/ci/build_index.py`'s record shape
and the corresponding Rust type in `crates/traverse-registry`
(`public_registry_state.rs`, governed today under the `055-registry-sync`
legacy passthrough, `specs/014-extraction-compatibility`) beyond what those
describe.

## Design Decisions

### New Index Record Fields

Each entry in `index.json`'s `capabilities` array gains three fields:

```json
{
  "namespace": "core",
  "id": "example-capability",
  "version": "1.0.0",
  "digest": "sha256:...",
  "artifact_url": "https://github.com/.../example-capability-1.0.0.wasm",
  "contract_digest": "sha256:...",
  "contract_url": "https://raw.githubusercontent.com/...",
  "deprecated": false,
  "summary": "One-line description of what this capability does.",
  "description": "Longer prose description from the contract.",
  "use_cases": [
    { "scenario": "As a <persona>, I want <goal>, so that <outcome>." }
  ]
}
```

- `summary` / `description`: copied verbatim from the contract's own
  top-level `summary`/`description` fields (already required, free-text,
  already public — spec `006-public-scope-and-identity` covers what
  `owner`/`namespace` may disclose, and neither field has ever been treated
  as sensitive). Empty string when absent on an older contract that predates
  either field being populated.
- `use_cases`: a sanitized projection of the contract's own `use_cases[]`
  array, keeping only `scenario` text per entry and dropping
  `input_example`, `output_example`, `happy`, and `persona_ref` — matching
  spec `116` FR-003's exclusion of "use-case inputs/outputs" exactly. An
  empty array when the contract has no `use_cases`.

No new privacy classification work is needed: `summary`/`description`/
`use_cases[].scenario` are exactly the fields this registry's own static
catalog (`scripts/ci/generate_catalog_pages.py`) already renders publicly
today for every capability — this spec makes explicit, structured,
single-fetch data out of content that was already public, not newly
disclosed.

### Not Included

`owner.contact` (already excluded from public catalog rendering per
`OWNER_GITHUB` mapping, `generate_catalog_pages.py`), `inputs.schema`/
`outputs.schema`, `persona_ref`, `service_type`, and every other contract
field stay out of this index extension. A consumer that needs them still
follows `contract_url` and fetches the full, digest-verified contract —
this spec only removes the *mandatory* per-capability fetch for the fields
spec `116`'s cache actually needs.

### Versioning

No `index.json` schema-version bump: `index_version` already increments on
every build regardless of content (`specs/003-index-release-pipeline`), and
these are purely additive fields — an existing consumer parsing only the
fields it knows about is unaffected. `PublicRegistryCapabilityRecord`
(Rust) gains the same three fields with `#[serde(default)]` so a consumer
built against a pre-this-spec crate version still deserializes a
post-this-spec `index.json`, and a post-this-spec crate deserializes a
pre-this-spec `index.json` (fields default to empty).

## Functional Requirements

- **FR-001**: `index.json` capability entries MUST include `summary`,
  `description`, and `use_cases` (each entry `{ "scenario": "<text>" }`
  only), sourced directly from the corresponding `contract.json`.
- **FR-002**: `use_cases` projection MUST exclude `input_example`,
  `output_example`, `happy`, and `persona_ref` — only `scenario` text
  survives.
- **FR-003**: A contract missing `summary`/`description`/`use_cases`
  entirely MUST NOT fail the build — the corresponding field is emitted as
  an empty string / empty array, consistent with `build_index.py`'s
  existing non-retroactive stance on older content.
- **FR-004**: `PublicRegistryCapabilityRecord` (`crates/traverse-registry`)
  MUST carry the same three fields, deserializable with `#[serde(default)]`
  for forward/backward compatibility with index generations that predate
  this spec.
- **FR-005**: `validate_public_registry_index` MUST NOT require
  `summary`/`description`/`use_cases` to be non-empty — absence is valid
  (see FR-003), not a validation failure.

## Success Criteria

- **SC-001**: A freshly built `index.json` entry for a capability with a
  populated `use_cases[]` carries only `scenario` text per entry — no
  `input_example`, `output_example`, `happy`, or `persona_ref` key present.
- **SC-002**: A capability contract lacking `summary`/`description`/
  `use_cases` still produces a valid index entry (empty string/array, no
  build failure).
- **SC-003**: `PublicRegistryCapabilityRecord` deserializes an `index.json`
  generation built before this spec landed (missing the three new fields)
  without error.

## Assumptions

- Building spec `116`'s actual offline metadata cache (atomic generation
  publication, staleness handling, incompatible-schema-version rejection)
  is traverse-side scope (`traverse#1132`/`#876`/`#1105`) and not
  implemented here — this spec only supplies the sanitized public input
  those consumers read.
- No change to `specs/005-yank-deprecation`'s yank/deprecation semantics:
  a deprecated version's entry carries the same three new fields as any
  other entry, consistent with how `contract_digest`/`contract_url` are
  already retained on deprecated entries per `specs/009`.

## Approval

Drafted by an agent from registry ticket #312's Definition of Done and
`traverse-framework/traverse` spec `116`'s already-approved FR-003. Per this
repo's no-self-approval-of-specs rule, this spec stays `Draft` pending the
repo owner's own explicit, standalone sign-off — the accompanying
implementation cites already-approved `009-contract-metadata-in-index` and
`055-registry-sync` for `spec-alignment` purposes in the meantime, the same
pattern decision-log entry 22 used for `specs/007-artifact-hosting`.
