# Cross-Repo Context

This repo's own docs (`docs/decision-log.md`, `specs/`) fully explain *this repo's* design. They do not, by themselves, tell you how this registry fits into the rest of the `traverse-framework` org, because that context lives in other repos this repo has no visibility into. This doc is the bridge — read it before planning any cross-repo-touching work.

**Rewritten 2026-07-28.** The previous version of this doc (three "open questions," all framed as unresolved) dated to 2026-07-06, this repo's first week. All three have since been resolved and implemented — the crate extraction landed, `registry sync` exists and works, and `reference-apps` consumes at least one component via `registry_ref`. This version replaces that historical framing with the current state and an explicit ownership matrix, per [#69](https://github.com/traverse-framework/registry/issues/69) section 5.2 (a real, recurring gap: agents have repeatedly misfiled work in the wrong repo this year — see the `#62` → `traverse#814` correction, and the `#58`/`#59` compatibility-bridge rework that started from an unverified claim).

## Org map

| Repo | Role |
|---|---|
| [`traverse-framework/traverse`](https://github.com/traverse-framework/traverse) | Runtime + CLI. Owns `traverse-contracts` (the schema everyone else depends on). Owns `traverse-cli` (`registry sync`/`capability publish`; a `list`/`search` browse UX is tracked there as [`traverse#814`](https://github.com/traverse-framework/traverse/issues/814), not here). No longer owns `traverse-registry` — extracted to this repo, see below. |
| [`traverse-framework/registry`](https://github.com/traverse-framework/registry) (this repo) | The external, git-based capability/event/workflow index (`capabilities/**/contract.json`, `index.json` releases) **and** the `traverse-registry` crate (`crates/traverse-registry/`, published independently to crates.io). Publish → validate → index → release is this repo's whole job; it does not run any CLI or resolve anything for a live app. |
| [`traverse-framework/reference-apps`](https://github.com/traverse-framework/reference-apps) | Real example Traverse apps (`traverse-starter`, `doc-approval`, `meeting-notes`, etc.) with real app manifests, component manifests, and capability contracts — the closest thing this org has to a real consumer. Owns its own UI shells; does not invent capability business logic (that lives in `traverse`'s example agents or a real product's own crate). |
| [`traverse-framework/.github`](https://github.com/traverse-framework/.github) | Shared governance: constitution, NFRs, quality standards, CLA. Every repo (including this one) points here instead of duplicating. |

## Ownership matrix (the concrete "which repo do I file this in" table)

| Concern | Owner repo | Notes |
|---|---|---|
| `capabilities/**` contracts, `index.json` releases, artifact (WASM) releases | **registry** | This repo's core content. Immutable once merged (spec 007); yank is additive (spec 005), never an edit. |
| `traverse-registry` crate source, its own crates.io release cadence | **registry** | `crates/traverse-registry/`. Independently versioned (decision-log entry 32) — not lockstep with `traverse`'s own workspace version. |
| `traverse-registry`'s consumers (anyone depending on the crate, e.g. `traverse` itself) | Whichever repo consumes it | `traverse`'s `Cargo.toml` pins it exact (`traverse-registry = "=<version>"`); bumping that pin is `traverse`-side work. |
| `traverse-cli registry sync` / `capability publish` / `registry list`/`search` | **traverse** | CLI mechanics live in `crates/traverse-cli`. Browse/search UX was misfiled here once (`#62`) before moving to `traverse#814` — the lesson: a request that's *about* the registry's content isn't automatically registry's *code* to write. |
| Embedder resolve / materialize (turning a `registry_ref` into a loaded, running capability at app-registration time) | **traverse** | Runtime-side resolution logic; registry only ever publishes the index/artifacts being resolved. |
| UI shells and `registry_ref` app manifests | **reference-apps** | Consumes the published index; does not define new capability contracts of its own unless it's genuinely a new reference app. |
| Shared governance (constitution, NFRs, CLA, spec-alignment gate script) | **`.github`** | Every repo pins a version via `.governance-version`; none forks it locally. |

If a ticket's actual implementation surface doesn't match the repo it was filed in, move it (open the equivalent in the right repo, close the original referencing the move) rather than force-fitting the work into the wrong codebase. That is what happened with `#62` → `traverse#814`.

## Current state (as of 2026-07-28)

- **Extraction: done.** `crates/traverse-registry` was physically moved from `traverse` into this repo (`#65`, `#9`). `traverse`'s own copy was deleted and its `Cargo.toml` now depends on the published crate exactly (`traverse-registry = "=0.9.1"` as of this writing) — independently confirmed by checking that `traverse-framework/traverse`'s `crates/traverse-registry/` path 404s and its `Cargo.toml` has no local path entry for it.
- **`registry sync` and `capability publish`: implemented**, in `traverse`'s CLI (`traverse` #542/#543 from the original open-question list). A synced index populates local workspace state; nothing resolves live against this repo at execution time (the zero-live-network-dependency principle from spec 001 holds).
- **`reference-apps` consumes via `registry_ref`**: at least `traverse-starter.process` resolves through the synced index rather than a local path (`reference-apps#97`). The other five published seed capabilities are still consumed via local-path/`TRAVERSE_REPO` in `reference-apps` as of the `#69` evidence snapshot (2026-07-27) — cutover for those is gated on the gap below, not on anything technical in the sync/resolve mechanism itself.
- **Known, current, top-priority gap** (`#69` section 1.1): all six published capabilities resolve to the identical stub-WASM digest — real content, not a placebo pipeline, but not yet real product binaries either. Flipping more `reference-apps` components to `registry_ref` before this closes would pin production manifests to stub artifacts. See `capabilities/README.md` and `#69` for the current, honest status.
- **Federation/governance compatibility**: the crate's governing-spec validation broke for any external consumer immediately after the first real publish (`#67`/`#68`, `traverse-registry` 0.9.0 → 0.9.1) — a structural bug in how the crate resolved its own governance data, unrelated to the index/sync mechanism above. Fixed, independently re-verified, and now regression-guarded by an isolated-consumer CI check (`#70`) that runs on every future release.

## Historical decisions (for provenance, not current status)

These three questions drove this repo's original design and are kept here only as a paper trail — do not treat them as open:

1. **Bundle `scope: public/private` vs. this repo's `namespace`/`owner`** — resolved as orthogonal axes (decision-log entry 20, `specs/006-public-scope-and-identity`, now Approved): `scope` is a resolution tier, `namespace`/`owner` is publisher identity, and "public" means exactly one thing — populated by `registry sync` from this repo.
2. **Nothing consumed from this registry** — resolved by the dual-mode `contract_path`/`registry_ref` manifest design (decision-log entry 21) and implemented by `traverse`'s `registry sync`/dual-mode resolution.
3. **Reference apps didn't reference this registry** — resolved by seeding real content here (decision-log entry 22) and `reference-apps#97`'s `registry_ref` adoption for `traverse-starter.process`.
