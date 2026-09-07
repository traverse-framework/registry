# Lifecycle Field Mapping

Per [#69](https://github.com/traverse-framework/registry/issues/69) section 4.4: this repo has three related but genuinely distinct "is this thing still good?" concepts, none of which is derived from or kept in sync with the others automatically. Confusing them is an easy mistake — this doc exists so nobody has to reverse-engineer the relationship from source again. Documentation only; nothing here changes any behavior.

## The three concepts

### 1. `contract.json`'s own `lifecycle` field

Every capability/event contract carries a `lifecycle` field (`traverse-contracts`' `Lifecycle` enum: `draft` / `active` / `deprecated` / `retired` / `archived`). It's set by the publisher **at publish time** and describes the contract author's own intent for that specific version.

**Critical property: it is effectively frozen once published.** `contract.json` is immutable after merge (spec 007; enforced by spec 002's CI checks) — a yank PR is explicitly required *not* to modify the existing `contract.json` (spec 005 FR-001/FR-002). So whatever `lifecycle` value a contract shipped with is what it says forever, regardless of what actually happens to that version later. In practice every one of this repo's 6 published capabilities says `"lifecycle": "active"`, unconditionally, because nobody publishes something as already-deprecated.

**Consequence**: `lifecycle` is not a live status field and is not how this repo marks a published version deprecated. Don't read it expecting it to reflect current reality for anything already merged.

### 2. `deprecated.json` sibling file + `index.json`'s `deprecated` flag (spec 005 — the actual yank mechanism)

The real, correct way to mark a specific published version deprecated: add a `deprecated.json` sibling file next to (never editing) the existing `contract.json`. The next index build sets that version's `deprecated: true` in `index.json` — verified directly against the live `index-v38` release, every current record correctly carries `"deprecated": false` by default. This is what a consumer's resolver is specified to check (range resolution skips `deprecated: true`; exact pins ignore it and still resolve) — see the [spec-status-matrix](spec-status-matrix.md) for why this mechanism is implemented and index-wired but not yet proven on real content (zero `deprecated.json` files exist anywhere in this repo as of this writing).

This is the only one of the three that's actually additive/immutability-safe by construction, and the only one this repo's own CI enforces.

### 3. `FederationApprovalState` (the extracted crate's own, narrower derivation — federation/peer-trust only)

Inside `crates/traverse-registry/src/federation.rs`, `approval_state_from_lifecycle()` maps concept #1 (the contract's frozen `lifecycle` field) into a `FederationApprovalState` used specifically for cross-peer federation trust decisions:

| `lifecycle` | `FederationApprovalState` |
|---|---|
| `draft` | `Draft` |
| `active` | `Approved` |
| `deprecated` | `Deprecated` |
| `retired` | `Rejected` |
| `archived` | `Rejected` |

This is a pure function of concept #1 — it inherits the same "frozen at publish time" limitation, and has **no connection at all** to concept #2 (`deprecated.json`/index flag). A version could have `lifecycle: active` (so `FederationApprovalState::Approved`) while also being yanked via `deprecated.json` (so `index.json` says `deprecated: true`) — both states can be simultaneously true and disagree, because nothing keeps them in sync. This is scoped narrowly to federation sync/trust logic inside the crate; it is not this repo's registry-wide deprecation mechanism and doesn't drive `index.json`.

## Practical rule of thumb

If you need to know whether a published capability version is **actually still current from this registry's point of view**, check `index.json`'s `deprecated` field (concept #2) — that's the one this repo's own publish/index pipeline keeps accurate. Treat `contract.json`'s `lifecycle` field and the crate's `FederationApprovalState` as point-in-time, publish-time-frozen signals, not live status.
