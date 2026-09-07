# Feature Specification: LLM-Assisted Authoring Assurance

**Feature Branch**: `023-authoring-assurance`
**Created**: 2026-09-06
**Status**: Draft
**Input**: Decided via `/brainstorm 341 355 365` with the repo owner. Full reasoning: `docs/decision-log.md` entry 85. Originating issue: `#355`. Deferred empirical-evaluation follow-on: a child issue of `#355`.

## Purpose

The registry publishes executable WASM. There is no published evidence that LLM-assisted capability authoring is reliable enough to be a supported path, and no precise boundary for what a reviewer must personally verify when an LLM produced the contract, the artifact source, or both.

`#355` bundled two things: a *policy* (classify LLM-assisted authoring by capability risk, state reviewer rules) and a full *empirical evaluation* (versioned corpus, controlled authoring run, measurement report on first-pass validation / security rejection / reviewer rework / defect escape / time-to-accepted). Nothing is currently blocked on the evaluation. This spec is the policy half only. It sets an interim, evidence-light boundary that reviewers can lean on today, and it lands the one provenance marker that cannot be reconstructed after the fact — which published capabilities took the LLM-assisted path.

The empirical evaluation is deferred to a child issue of `#355` that activates on real demand. Its methodology, not this spec, sets the bar for promoting the low-risk tier from `experimental` to `supported`.

## Scope

In scope:

- a required `authoring.method` field on every newly ADDED or CHANGED `capabilities/<namespace>/<id>/<version>/contract.json`
- a forward-only, diff-based CI gate in `scripts/ci/capability_validation.py` enforcing that field, in the existing `check_new_*` family
- an interim product stance for LLM-assisted authoring, by capability risk tier (D1 of decision 85)
- a reviewer-qualification rule splitting what any reviewer may approve from what requires a qualified Rust/security reviewer (D3 of decision 85)

Out of scope:

- the empirical evaluation corpus, the controlled authoring run, and the measurement report (deferred child issue of `#355`) — including the numeric criteria for promoting the low-risk tier to `supported`
- retroactively adding `authoring.method` to already-published immutable contracts (impossible without editing immutable content) — the gate is forward-only, the same treatment `#140` / spec 017 FR-004 gave earlier field additions
- enabling LLM-assisted authoring for high-risk / effectful capability classes (prohibited here; requires separate evidence and its own recorded decision)
- a formal, objective roster or credentialing process for "qualified Rust/security reviewer" — this spec names the role and its responsibilities; near-term the repo owner is the qualified reviewer until more are designated
- any bypass of the existing verification, permission, placement, signature, or provenance gates — a generated artifact earns no special trust from being declared

## Requirements

### Functional Requirements

- **FR-001**: Every newly ADDED or CHANGED `contract.json` MUST carry a top-level `authoring.method` value of exactly one of `human` | `llm-assisted`. `human` means no LLM materially produced the contract or the artifact source. `llm-assisted` means an LLM materially produced any part of the contract or the artifact source, regardless of how much a human then edited it.
- **FR-002**: `scripts/ci/capability_validation.py` MUST fail a PR whose newly ADDED or CHANGED `contract.json` is missing `authoring.method` or carries a value outside the FR-001 enum. The check is diff-based over `capabilities/` (only files added or modified in the PR's range), added as a `check_new_contracts_declare_authoring_method` sibling to `check_new_contracts_have_artifact_reference` / `check_new_use_cases_have_persona_ref` / `check_new_contracts_action_enum_coverage`, so already-published immutable versions are never re-litigated.
- **FR-003**: A contract declaring `authoring.method: "llm-assisted"` MUST also record, under `authoring`, links sufficient to audit the candidate: the artifact source revision, the artifact digest (already required by `#187`), the test evidence (CI run or equivalent), and the human review decision (approving reviewer + PR). Where a value is already carried elsewhere on the contract (e.g. `artifact.digest`), a reference to it satisfies this requirement rather than a copy.
- **FR-004**: The interim product stance for LLM-assisted authoring is tiered by capability risk:
  - **low-risk, deterministic classes** — allowed, labelled `experimental`, and tracked. Promotion to `supported` is out of scope for this spec and is gated on the deferred empirical evaluation's evidence plus a later standalone owner decision recorded in `docs/decision-log.md`.
  - **bounded medium-risk classes** — case-by-case only; not a standing allowance. Each instance is an explicit reviewer decision recorded on the PR.
  - **high-risk / effectful classes** — prohibited while this spec is in force.
- **FR-005**: Review of an `llm-assisted` capability is split:
  - a reviewer without qualified Rust/security standing MAY independently approve **intent / contract evidence**: contract shape, `use_cases` surface coverage, `persona_ref` resolution, documentation, and semver classification.
  - a **qualified Rust/security reviewer** MUST approve **implementation / security evidence**: the artifact source, the WASM artifact, and test adequacy. This requirement is NOT waivable by risk tier — it applies to every `llm-assisted` capability, low-risk included.
  - "qualified Rust/security reviewer" is a named role; until a broader roster is designated, the repo owner fills it.
- **FR-006**: An `llm-assisted` capability MUST pass every gate a `human`-authored capability passes (schema, semver-vs-diff, digest, namespace collision, dependency resolvability, `use_cases` surface coverage, persona resolution, artifact signature). Declaring `authoring.method` grants no exemption from any of them.

## Success Criteria

- **SC-001**: A newly added or changed `contract.json` missing `authoring.method`, or with a value outside the enum, is rejected by CI before merge.
- **SC-002**: Every current-version contract added after this spec's approval carries a valid `authoring.method`.
- **SC-003**: Every `llm-assisted` contract carries the FR-003 audit links (directly or by reference).
- **SC-004**: No high-risk / effectful capability is published with `authoring.method: "llm-assisted"` while this spec is in force.
- **SC-005**: For every merged `llm-assisted` capability, the PR record shows a qualified Rust/security reviewer approved the implementation/security evidence.

## Governing Relationship

This spec is additive to `001-registry-foundation`. It does not amend FR-011 or any other `001` text; it adds a new required field on `capabilities/` contracts and a new CI gate, plus a review-process and product-stance policy — the same layering discipline `006-public-scope-and-identity` used for `owner`/`namespace` and `017-persona-registry` used for `persona_ref`. It governs `capabilities/` and `scripts/ci/capability_validation.py`.

The interim risk-tier stance in FR-004 is explicitly provisional. The deferred child evaluation issue of `#355` produces the evidence; a later, standalone owner decision (recorded in `docs/decision-log.md`, same process as every prior spec change) is what may move the low-risk tier from `experimental` to `supported`. Per decision 75 / decision 83 Q9, this decision-log entry is the ADR the `#355` comment asked for — this repo keeps no separate `docs/adr/` tree.
