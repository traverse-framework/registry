# Spec Status Matrix

Per [#69](https://github.com/traverse-framework/registry/issues/69) section 7.3: a single table distinguishing **approved** (governance state), **implemented** (code/CI exists), and **proven on real content** (actually exercised end-to-end, not just theoretically correct). This repo has had at least one case of "spec approved, code implemented, but nobody checked whether real content actually satisfies it" — spec 008's own `FR-004` is currently violated by every published capability (see below) — so this table exists to make that class of gap visible at a glance instead of rediscovering it by accident.

All data below independently verified against the live repo/release state on 2026-07-28, not copied from spec text. Re-verify before trusting an old copy of this table.

| Spec | Approved? | Implemented? | Proven on real content? | Notes |
|---|---|---|---|---|
| `001-registry-foundation` | Yes | Yes | Yes | Whole pipeline (publish → validate → index → release) runs and has shipped multiple real releases. |
| `002-capability-validation` | Yes | Yes | Yes | `scripts/ci/capability_validation.py` validates all 6 real published capabilities. |
| `003-index-release-pipeline` | Yes | Yes | Yes | `index-v38` (and 37 before it) are real, published GitHub Releases with the documented schema. |
| `004-ai-advisory-review` | Yes | Yes, dormant by design | **No** | Intentionally never runs for real (no `ANTHROPIC_API_KEY`, decisions 19/25) — `DEGRADED_COMMENT` posts instead. Not a bug; don't "fix" by provisioning a key or reviving subscription auth. |
| `005-yank-deprecation` | Yes | Yes (format spec'd, index carries a `deprecated` field) | **No** | Verified: zero `deprecated.json` files exist anywhere in this repo. The index's `deprecated` field is wired and defaults correctly (`false` on every current record) but the actual yank workflow has never been exercised on real content. |
| `006-public-scope-and-identity` | Yes | Yes | Yes | All 6 published capabilities carry real `owner`/`namespace` fields validated by `002`'s CI job. |
| `007-artifact-hosting` | Yes | Yes (hosting mechanism) | **Partially** | The release/digest/URL hosting mechanism itself works and is proven (real GitHub Release assets, real digests recorded correctly). What's *not* proven: the artifacts being hosted are 6 identical 36-byte stubs, not real product binaries — the mechanism is sound, the content routed through it isn't yet real. |
| `008-reference-capability-publication` | Yes | Yes | **No — `FR-004` currently violated** | `FR-004`: "Published digests MUST match Traverse example WASM artifacts." Verified: all 6 published capabilities share the identical digest `sha256:5647c39a...`, a placeholder, not a real Traverse example artifact match. This is `#69` section 1.1, the top-priority open gap. |
| `009-contract-metadata-in-index` | Yes | Yes | Yes (registry side) | Verified directly: `index-v38`'s records carry real `contract_digest`/`contract_url` fields pointing at actual commit-pinned raw content. Consumer-side proof (Traverse's `registry_ref` path actually reading and checking these) is `traverse`-side, out of this repo's ability to verify. |
| `010-crate-publish-pipeline` | Yes | Yes | Yes | Three real publishes shipped: `0.0.1`, `0.9.0`, `0.9.1`, all via the tag-triggered pipeline, all confirmed live on crates.io. |
| `011-capability-registry-adoption` / `012-event-registry-adoption` | Yes | Yes | Yes | Verbatim-adopted crate content builds and its full test suite (159 unit + 87 + 42 integration) passes. |
| `013-inherited-registry-governance` | Yes | Yes | Yes | Blanket-governs the rest of the extracted crate's behavior as-is; crate builds and tests pass unchanged post-extraction. |
| `014-extraction-compatibility` | Yes | Yes | Yes | The exact mechanism it exists to fix (governing-spec resolution for an external consumer) is now regression-guarded by `#70`'s isolated-consumer CI check, run against the real published crate. |

## Legacy passthrough entries (not this repo's own specs)

`002-capability-contracts`, `005-capability-registry`, `007-workflow-registry-traversal`, `011-event-registry`, `037-semver-range-resolution`, `039-connector-plugin-architecture`, `043-module-dependency-management`, `044-application-bundle-manifest`, `046-public-cli-app-registration`, `055-registry-sync` are minimal passthrough registrations (`specs/014-extraction-compatibility/legacy-ids/`), not specs this repo authors or maintains requirements for — they exist solely so the extracted crate's compiled-in governing-spec IDs keep validating after relocation. They don't belong in the proof matrix above; there's no "implemented here" or "proven here" to assess.

## Known-real, currently-open gaps this table surfaces

1. **`008` `FR-004` is violated today** — every published capability is a stub. See `#69` section 1.1.
2. **`005` (yank) has never been exercised on real content** — the mechanism is spec'd and index-wired but unproven end-to-end. See `#69` section 4.1.
3. **`004` (AI advisory) stays intentionally dormant** — not a gap, a deliberate, already-decided tradeoff (decisions 19/25). Listed here only so it isn't mistaken for an oversight.
