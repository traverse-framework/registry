# Feature Specification: Sanitized Display Metadata in the Public Sync Index

**Feature Branch**: `claude/issue-312-public-metadata-projection`
**Amended by**: `claude/issue-318-search-projection-fields`
**Created**: 2026-08-25
**Status**: Draft
**Input**: Registry ticket #312 — supply the verified, public-only input for
`traverse-framework/traverse` spec `116-verified-public-contract-metadata-cache`
(Approved 2026-08-24) FR-003. Amended for registry ticket #318 — supply the
same for `traverse-framework/traverse` spec `114-mcp-capability-search`
(Approved 2026-08-24) FR-005.

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

`owner` (both `owner.team` and `owner.contact` — `owner.contact` is already
excluded from public catalog rendering per `OWNER_GITHUB` mapping,
`generate_catalog_pages.py`, and neither field is needed by spec `114`'s
FR-005 projection), `inputs.schema`/`outputs.schema`, and `persona_ref` stay
out of this index extension. A consumer that needs them still follows
`contract_url` and fetches the full, digest-verified contract — this spec
only removes the *mandatory* per-capability fetch for the fields spec `116`'s
cache and spec `114`'s search tool actually need.

## Amendment (registry#318): Search-Projection Fields

`traverse-framework/traverse` spec `114-mcp-capability-search` (Approved
2026-08-24) FR-005 requires its `search_capabilities` MCP tool's result
records to carry, beyond identity/version/display metadata already covered
above, "service type, permitted targets, and public lifecycle/provenance
metadata" — and explicitly prohibits raw contracts, use-case examples,
private records, or secret material, the same boundary this spec already
draws.

Checked first, before adding anything: `service_type` was originally listed
under "Not Included" above because nothing needed it yet; spec `114` now
does. `permitted_targets`, `lifecycle`, and `provenance` were never
evaluated for inclusion or exclusion — they simply weren't asked for by spec
`116`.

### New Index Record Fields (Amendment)

Each entry in `index.json`'s `capabilities` array gains four more fields:

```json
{
  "service_type": "stateless",
  "permitted_targets": ["local", "cloud", "edge", "device"],
  "lifecycle": "active",
  "provenance": {
    "source": "greenfield",
    "author": "enricopiovesan",
    "created_at": "2026-07-08T00:00:00Z",
    "spec_ref": "058-workflow-pipeline-execution@1.0.0",
    "adr_refs": ["0001-rust-wasm-foundation"],
    "exception_refs": []
  }
}
```

- `service_type` / `permitted_targets`: copied verbatim from the contract's
  own top-level fields (`traverse-contracts::ServiceType` /
  `Vec<ExecutionTarget>`) — already required on every published contract,
  already rendered on this registry's own public service-type catalog pages
  (decision-log entry 54), never treated as sensitive.
- `lifecycle`: copied verbatim from the contract's own top-level `lifecycle`
  field (`traverse-contracts::Lifecycle`: `draft`/`active`/`deprecated`/
  `retired`/`archived`). Distinct from the index's existing boolean
  `deprecated` flag (spec `005-yank-deprecation`, derived from a sibling
  `deprecated.json`, not from this field) — both are carried because spec
  `114` FR-005 asks for "lifecycle" and nothing here supersedes spec 005's
  established yank signal.
- `provenance`: the contract's own top-level `provenance` object, passed
  through unfiltered. Checked directly against every currently-published
  contract (`capabilities/*/*/*/contract.json`) before deciding this: its
  fields (`source`, `author`, `created_at`, `spec_ref`, `adr_refs`,
  `exception_refs`) hold governance/attribution metadata only — `author`
  values observed today are GitHub handles or team names (e.g.
  `enricopiovesan`, `platform-team`), never an email address or other PII;
  no field ever contains a secret. This is a different object from the
  contract's `owner` field (which does carry `owner.contact`, an email, and
  stays excluded per "Not Included" above) — the two must not be conflated.
  `null` when a contract predates `provenance` being populated.

### Functional Requirements (Amendment)

- **FR-006**: `index.json` capability entries MUST include `service_type`,
  `permitted_targets`, `lifecycle`, and `provenance`, sourced directly from
  the corresponding `contract.json` top-level fields of the same names.
- **FR-007**: `provenance` MUST be passed through unfiltered (no sub-field
  redaction) — every one of its fields is already public governance
  metadata, none is a secret or PII (see analysis above). This is a
  deliberate departure from the `use_cases` sanitization in FR-002: that
  field required filtering because `input_example`/`output_example` can
  carry arbitrary business data; `provenance`'s fixed field set does not.
- **FR-008**: A contract missing `service_type`, `permitted_targets`,
  `lifecycle`, or `provenance` entirely MUST NOT fail the build — the
  corresponding field is emitted as an empty string / empty array / `null`,
  consistent with FR-003's non-retroactive stance.
- **FR-009**: `PublicRegistryCapabilityRecord` (`crates/traverse-registry`)
  MUST carry the same four fields, deserializable with `#[serde(default)]`
  for forward/backward compatibility with index generations that predate
  this amendment.

### Success Criteria (Amendment)

- **SC-004**: A freshly built `index.json` entry carries `service_type`,
  `permitted_targets`, `lifecycle`, and an unfiltered `provenance` object
  matching the source contract exactly.
- **SC-005**: A capability contract lacking any of the four fields still
  produces a valid index entry (empty string/array/`null`, no build
  failure).
- **SC-006**: `PublicRegistryCapabilityRecord` deserializes an `index.json`
  generation built before this amendment landed (missing all four fields)
  without error.

### Versioning

No `index.json` schema-version bump: `index_version` already increments on
every build regardless of content (`specs/003-index-release-pipeline`), and
every field this spec adds — the original three plus the amendment's four —
is purely additive; an existing consumer parsing only the fields it knows
about is unaffected. `PublicRegistryCapabilityRecord` (Rust) gains the same
fields with `#[serde(default)]` so a consumer built against a pre-this-spec
crate version still deserializes a post-this-spec `index.json`, and a
post-this-spec crate deserializes a pre-this-spec (or pre-amendment)
`index.json` (fields default to empty/`null`).

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
  those consumers read. The same is true of spec `114`'s
  `search_capabilities` MCP tool itself (query matching, scoring, the
  result envelope's `stale`/error semantics) — this amendment only supplies
  the public record fields FR-005 requires that field to contain.
- No change to `specs/005-yank-deprecation`'s yank/deprecation semantics:
  a deprecated version's entry carries the same new fields as any other
  entry, consistent with how `contract_digest`/`contract_url` are already
  retained on deprecated entries per `specs/009`.

## Approval

Drafted by an agent from registry ticket #312's Definition of Done and
`traverse-framework/traverse` spec `116`'s already-approved FR-003, amended
by an agent from registry ticket #318's Definition of Done and
`traverse-framework/traverse` spec `114`'s already-approved FR-005. Per this
repo's no-self-approval-of-specs rule, this spec stays `Draft` pending the
repo owner's own explicit, standalone sign-off — the accompanying
implementation cites already-approved `009-contract-metadata-in-index` and
`055-registry-sync` for `spec-alignment` purposes in the meantime, the same
pattern decision-log entry 22 used for `specs/007-artifact-hosting`.
