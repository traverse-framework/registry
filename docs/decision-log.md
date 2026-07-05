# Registry Design Decision Log

Status: historical record of the brainstorm that shaped `specs/001-registry-foundation/spec.md`. This document is not itself governing — if it and the spec disagree, the spec wins. Kept for provenance: why the shape is what it is, not just what the shape is.

Date: 2026-07-03

## Context

Traverse (the runtime) already had an in-repo registry engine (`crates/traverse-registry` in `traverse-framework/traverse`) covering public/private overlay resolution, semver rules, dependency locking, connector registration, and composability metadata (governed by that repo's specs 005, 007, 011, 034–037, 039, 041, 043). That engine already assumed a "public registry" existed as a data source, but nothing actually served that role. This log captures the decisions made to stand up that role as a new, dedicated repo: `traverse-framework/registry`.

## Decisions

1. **Split boundary**: `traverse-contracts` stays in `traverse-framework/traverse`; `traverse-registry` (the crate, plus all capability content) moves to this repo. Rationale: `traverse-contracts` is the schema every consumer (runtime, CLI, MCP, this registry) depends on symmetrically — one canonical home avoids two repos both claiming to define "the schema." See `traverse/specs/051-registry-extraction/spec.md` for the formal migration record.

2. **CLA**: required org-wide, starting now (before this repo or `traverse` accumulates outside contributors), to preserve the option to relicense or commercially license the codebase later without needing retroactive consent from every contributor. Governed centrally in `traverse-framework/.github`.

3. **Architecture — git-based static index, not a hosted service, for v1**: capability records are files in this repo, validated and quality-gated by CI, merged via reviewed PRs — the same shape as crates.io's index repo or Homebrew-core. Chosen because: (a) it's free to run (GitHub Actions is free for public repos — no server, no database, no bill regardless of usage), and (b) it fits the existing spec-gated PR governance model this org already uses. Explicitly designed so a hosted API layer can be added later without a breaking schema migration — the resolve/fetch interface and the record schema are meant to be storage-agnostic from day one.

4. **Future hosted layer, when built**: serverless/edge, pay-per-use. Read path (resolving/fetching capabilities — the overwhelming majority of traffic in any registry) sits behind a CDN/edge cache; write path (publishing) runs on pay-per-request compute; artifact storage uses cheap object storage. The goal: running cost scales with actual usage, never a fixed bill for idle capacity.

5. **Schema reserves `owner` and `namespace` fields now**, defaulted to a "core" identity and an unscoped namespace, even though the only accepted publisher in v1 is the core team via PR review. Rationale: adding real third-party/dev publishing later without these fields would require a breaking migration of every existing record; reserving them now costs almost nothing.

6. **Sync/consume model**: `traverse-cli registry sync` (CLI, lives in `traverse-framework/traverse`) fetches the latest published index into local durable workspace state. The runtime (`traverse-registry` crate, wherever it ends up being consumed from) only ever resolves against that local synced state — zero live network dependency at execution time. This reuses the exact pattern `traverse`'s spec 046 already established for app registration (CLI writes durable local state, runtime reads it, never calls a live service).

7. **Publish mechanism**: merging a PR to `main` in this repo triggers CI to build a versioned aggregated index artifact, published as a GitHub Release. `sync` fetches that artifact, not the raw git tree. This reuses the shape of `traverse`'s existing spec 048 (tag → build → publish pipeline for its own crates), rather than inventing a new mechanism.

8. **Quality gate — deterministic + AI-advisory, not AI-blocking**: schema validation, semver-bump-vs-actual-diff checks, digest integrity, namespace-collision checks, and dependency-resolvability checks are deterministic and block merge. A separate AI pass flags likely semantic duplicates (capabilities that overlap in effect but not in name/tags) and capability-boundary quality issues (e.g. a CRUD wrapper disguised as a capability) as PR comments — advisory only. This is required to be advisory, not blocking, because the constitution (Principle II, inherited from `traverse-framework/.github`) already requires explicit manual approval before any contract publication; the AI pass informs that human review, it doesn't replace it.

9. **CLI automates the PR flow**: `traverse-cli capability publish` (implemented in `traverse-framework/traverse`, not here) validates a contract locally, computes the artifact digest, and uses git + `gh` to open a PR against this repo automatically — the equivalent of Homebrew's `brew bump-formula-pr`. The human review/approval gate on the PR is unchanged; only the mechanical git steps are automated.

10. **Repo layout**: `capabilities/<namespace>/<id>/<version>/contract.json` (+ artifact references alongside). Path-encodes identity so two capabilities structurally cannot occupy the same path (namespace collisions become impossible, not just checked), and a new version is always a new file — the filesystem itself enforces the immutability rule already required by `traverse`'s spec 005, instead of relying purely on CI logic to catch violations.

11. **Artifact storage**: WASM binaries are **not** committed into this git repo directly (git handles binary blobs and unbounded history growth badly, which actively fights an append-only immutable-version model). They live as GitHub Release assets; the git-tracked contract stores only a digest + release URL. Free to host, keeps the repo itself small and fast indefinitely.

12. **Deprecation**: a `cargo yank`-style mechanism — a version is never edited or deleted, but a separate deprecation-flag file marks it excluded from range resolution (`^1.2.0`-style) while still resolvable by an exact pin. This is the standard, well-proven shape every major package registry has converged on for exactly this problem, and it doesn't touch immutability.

13. **Tracking**: this repo gets its own GitHub Project board (Project 3, "Registry" — already existed, needed cleanup of 7 stray unrelated `traverse` issues that had been swept in incorrectly).

14. **Semver rules**: inherited as-is from `traverse`'s specs 005/037/043, moving with the crate rather than being redesigned.

15. **Repo naming across the org** (`traverse`, `reference-apps`, `website`, `registry` — lowercase-kebab-case, matching the dominant OSS convention and the org's existing crate-naming style) — decided in the same session, but tracked separately in `traverse/docs/repo-migration-plan.md`, not part of this repo's scope.

16. **Governance consolidation**: constitution, NFRs, quality standards, antipatterns, compatibility policy, exception process, CLA, and the spec-alignment CI script were pulled out of `traverse-framework/traverse` into a new shared repo, `traverse-framework/.github`, rather than copied into this repo. Every repo (including this one) keeps a thin pointer to a pinned governance version instead of a duplicated copy — see `.specify/memory/constitution.md` in this repo.

17. **AI-advisory review tooling refinement, then reverted (see 18)**: briefly switched the pass to a `.claude/skills/capability-review` Claude Code skill invoked via `claude -p` in CI, authenticated with a subscription token (`CLAUDE_CODE_OAUTH_TOKEN`, from `claude setup-token`) instead of a metered `ANTHROPIC_API_KEY`. The intent was avoiding per-call API billing when the org already carries a Claude subscription.

18. **Reverted decision 17 — subscription auth shares a real quota with personal usage**: `CLAUDE_CODE_OAUTH_TOKEN` authenticates as the account's actual Claude subscription, and Pro/Max/Team plans share **one unified usage limit across all Claude Code access** — interactive terminal sessions, IDE usage, and any CI job using that same token all draw from the same pool. For the repo owner, who actively depends on that plan for real interactive work, this meant CI runs could plausibly push personal usage into hitting plan limits sooner, or during, active interactive sessions. Reverted back to the original metered `ANTHROPIC_API_KEY` (Claude Console workspace key) design: costs isolated, billed separately, zero risk of touching personal usage. The `capability-review` skill file was removed since it's not used by the reverted implementation; the prompt logic lives inline in `scripts/ci/ai_advisory_review.py` instead. Spec 004's FR-001–004 are unaffected by this reversal, same as decision 17 — no normative requirement changed either way.

19. **`ANTHROPIC_API_KEY` deliberately not provisioned (as of 2026-07)**: after reverting decision 17, the repo owner explicitly said they don't want to allocate budget for a metered `ANTHROPIC_API_KEY` right now. The `ai-advisory-review` CI job is left wired up and correct (see `scripts/ci/ai_advisory_review.py`) but runs dormant: with no secret configured, `call_claude` raises immediately and every PR touching `capabilities/**/contract.json` gets the `DEGRADED_COMMENT` advisory notice instead of a real review. This is expected, not a bug -- do not "fix" it by requesting a key, by reintroducing subscription-token auth (see decision 18 for why that was reverted), or by making the job block merges to compensate. It stays this way until the repo owner decides to fund it.

## What Was Explicitly Deferred, Not Decided

- The exact schema field names/types for `owner`/`namespace` (reserved conceptually, not yet finalized in code)
- The hosted API layer's concrete implementation (deferred until real usage justifies it)
- Third-party publisher onboarding flow (namespace claiming, identity verification)
