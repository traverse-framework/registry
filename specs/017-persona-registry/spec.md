# Feature Specification: Persona Registry

**Feature Branch**: `017-persona-registry`
**Created**: 2026-08-08
**Status**: Approved
**Input**: Decided via `/brainstorm` with the repo owner. Full reasoning: `docs/decision-log.md` entry 53.

## Purpose

`use_cases[].scenario` (spec 001 FR-011, amended by decision 46 to require full user-story format) embeds a persona name as free text inside each scenario sentence -- e.g. "As a productivity app developer, I want...". Auditing every persona phrase currently in use across the 11 published capabilities' 31 `use_cases` found real overlap: three different phrasings across `traverse-starter`'s `process`/`validate`/`summarize` describe the same imagined person building one note-taking app.

This spec introduces `personas/` as a real, governed content type -- the same immutable/versioned model `capabilities/` already uses -- and requires every `use_case` to reference exactly one persona by id, so "which persona does this use case serve" is answerable from the contract itself, not from parsing prose.

## Scope

In scope:

- `personas/<persona-id>/<version>/persona.json`, immutable once published, same model as `capabilities/`
- a required `persona_ref` field on every `use_cases[]` entry, naming a real, registered persona id
- a required `distinguished_from` field on every persona, naming every other persona it could plausibly be confused with and how it differs
- diff-based CI enforcement (mirroring the #140 scenario-format pattern) that newly-added `use_cases` entries carry a `persona_ref` resolving to a real, approved persona

Out of scope:

- retroactively enforcing `persona_ref` on already-published, immutable `use_cases` entries from before this spec (impossible without editing immutable content) -- existing entries are republished at a patch bump instead (see decision 53's execution boundary), the same treatment decision 46 gave the scenario-format rewrite
- a persona claiming/ownership model for third-party publishers (out of scope the same way namespace claiming is, per spec 001's own stated assumption)
- allowing a `use_case` to reference more than one persona -- the existing "As a `<persona>`..." phrasing is inherently single-persona, and `persona_ref` mirrors that

## Requirements

### Functional Requirements

- **FR-001**: A persona record MUST live at `personas/<persona-id>/<version>/persona.json`, `<persona-id>` MUST be a kebab-case slug, and `<version>` MUST be exact semver -- identical layout rules to `capabilities/` (spec 001 FR-001/FR-002 equivalents).
- **FR-002**: A persona record MUST contain: `id` (matching its path segment), `version` (matching its path segment), `name` (a short human-readable label), `summary` (one sentence), `description` (a fuller paragraph -- the actual context, goals, and constraints that make this persona a specific person, not a generic role), and `distinguished_from` (an array of `{persona_id, how}`, non-empty whenever at least one other persona is registered).
- **FR-003**: A persona record, once merged, MUST NOT be edited -- identical immutability rule to `capabilities/<namespace>/<id>/<version>/contract.json` (spec 001's own immutability rule). A correction is a new version, never an edit.
- **FR-004**: Every `use_cases[]` entry (spec 001 FR-011) MUST carry a `persona_ref` field naming a real, registered `personas/<persona-id>` -- enforced going forward, diff-based (only newly-added `contract.json` files in a PR are checked, mirroring `scripts/ci/capability_validation.py`'s existing `check_new_scenarios_are_user_stories` pattern from #140) so already-published immutable versions are never re-litigated.
- **FR-005**: `persona_ref` MUST resolve to a persona id that exists in `personas/` with at least one `approved`-equivalent (published) version at check time. A `use_case` referencing an unregistered persona id MUST fail CI.
- **FR-006**: `distinguished_from` entries MUST reference real, registered persona ids -- a dangling reference (a typo, or a persona that was renamed) MUST fail CI the same way `persona_ref` does.

## Success Criteria

- **SC-001**: Every `use_cases[]` entry across every current-version capability contract carries a valid `persona_ref`.
- **SC-002**: Every registered persona's `distinguished_from` list is non-empty and every entry in it resolves to a real persona id.
- **SC-003**: A new `contract.json` with a `use_case` missing `persona_ref`, or naming an unregistered persona, is rejected by CI before merge.
- **SC-004**: A new `persona.json` missing `distinguished_from`, or with a dangling reference in it, is rejected by CI before merge.

## Governing Relationship

This spec is additive to `001-registry-foundation` (which reserved `use_cases` via FR-011) -- it does not amend FR-011's text, it adds a new, independently governed content type and a new required field on an existing one, the same layering discipline `006-public-scope-and-identity` used for `owner`/`namespace`.
