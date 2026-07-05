# Cross-Repo Context

This repo's own docs (`docs/decision-log.md`, `specs/`) fully explain *this repo's* design. They do not, by themselves, tell you how this registry fits into the rest of the `traverse-framework` org, because that context lives in other repos this repo has no visibility into. This doc is the bridge -- read it before planning any v1-completeness work (real capability content, consumer-side integration, anything that touches how Traverse apps actually get capabilities from here).

## Org map

| Repo | Role |
|---|---|
| [`traverse-framework/traverse`](https://github.com/traverse-framework/traverse) | Runtime + CLI. Owns `traverse-contracts` (the schema). Also, today, still owns the `traverse-registry` crate pending extraction (`traverse` issue tracking this: see `specs/051-registry-extraction/spec.md` there, and this repo's issue #9). |
| [`traverse-framework/registry`](https://github.com/traverse-framework/registry) (this repo) | The external, git-based capability index: publish/validate/index/release pipeline. Does not itself consume capabilities -- nothing reads from it yet (see "Open question 2" below). |
| [`traverse-framework/reference-apps`](https://github.com/traverse-framework/reference-apps) (formerly `App-References`) | Real example Traverse apps (`traverse-starter`, `trace-explorer`) with real app manifests, component manifests, and capability contracts -- the closest thing to a "real consumer" this org has today. |
| [`traverse-framework/.github`](https://github.com/traverse-framework/.github) | Shared governance: constitution, NFRs, quality standards, CLA. Every repo (including this one) points here instead of duplicating. |

## Open question 1: does this registry's "public" collide with the existing bundle `scope: public/private`?

The **existing, in-repo** `traverse-registry` crate (in `traverse-framework/traverse`, pending extraction into this repo per issue #9) already has its own notion of bundle visibility, documented in `traverse`'s [`docs/registry-bundle-authoring-guide.md`](https://github.com/traverse-framework/traverse/blob/main/docs/registry-bundle-authoring-guide.md):

> `scope` | Yes | `public` or `private`. Public bundles are visible to all consumers; private bundles are registry-scoped.

This predates this repo. It is not yet decided whether:
- a bundle's `scope: public` is meant to mean "resolvable via `traverse-framework/registry`" (this repo), or
- it means something narrower and unrelated -- e.g. visibility across workspaces on a single local runtime instance, with no connection to this repo at all.

This repo's own schema separately reserves `owner`/`namespace` fields (decision 5, `specs/001-registry-foundation`) for a *different* kind of scoping (who published it / what namespace it lives in) -- that is not the same axis as the bundle-level `public`/`private` `scope` field above. Conflating the two, or leaving them silently unreconciled, is a real risk for whoever writes the next spec here.

**This needs to be resolved in a spec (in whichever repo ends up owning the concept) before further schema evolution here.**

## Open question 2: nothing consumes from this registry yet

Two CLI commands were decided during this repo's original brainstorm (decision log items 6 and 9) but, until 2026-07-05, had no ticket anywhere:

- `traverse-cli registry sync` -- fetch the published index, write local durable state. Now tracked: [`traverse` issue #542](https://github.com/traverse-framework/traverse/issues/542).
- `traverse-cli capability publish` -- automate opening a PR against this repo. Now tracked: [`traverse` issue #543](https://github.com/traverse-framework/traverse/issues/543).

Both are `needs-spec` and unstarted. Until at least `sync` exists, this repo's entire pipeline (publish -> validate -> index -> release) has no consumer.

## Open question 3: reference apps don't reference this registry today

Real, working capability contracts already exist and would pass this repo's validation as-is -- e.g. `traverse`'s [`contracts/examples/traverse-starter/capabilities/process/contract.json`](https://github.com/traverse-framework/traverse/blob/main/contracts/examples/traverse-starter/capabilities/process/contract.json) has real `namespace`/`id`/`version`/`owner` fields, not placeholders.

But `reference-apps`' component manifests resolve capabilities by local relative file path today (see `manifests/traverse-starter/components/process/component.manifest.json`'s `contract_path` field), not by namespace/id/version lookup against a synced registry index. There is no spec or ticket anywhere describing:
- whether/how reference-apps' capability resolution should switch to registry-sourced lookups once `registry sync` exists,
- or whether the first real "published content" in this repo's `capabilities/` directory should simply be these existing example contracts, republished here as the seed content proving the pipeline end-to-end.

## Why this matters for planning here

Specs 001-005 in this repo are complete and internally consistent for the pipeline itself, but they were designed without cross-referencing the three items above -- because that context lives in other repos this repo can't see. Before treating "v1 registry serving Traverse apps" as fully planned, these three open questions need their own resolution (likely their own spec slice(s), possibly split across this repo and `traverse`), not just an assumption that the existing specs already cover them.
