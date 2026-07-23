# Traverse Registry

The public capability registry for [Traverse](https://github.com/traverse-framework/traverse) — a git-based, CI-validated, PR-published registry of capability contracts and their artifacts.

## What This Is

Traverse's runtime resolves capabilities through `traverse-registry` (a crate that lives here — see `specs/051-registry-extraction/spec.md` in the `traverse` repo for the migration record). That crate needs somewhere real to resolve capabilities *from*. This repo is that place: capability records live here as reviewed, versioned files; CI enforces quality and immutability; publishing a capability means opening a PR.

Read [`specs/001-registry-foundation/spec.md`](specs/001-registry-foundation/spec.md) for the full governing spec, and [`docs/decision-log.md`](docs/decision-log.md) for the reasoning behind every major decision.

## How Publishing Works

1. A capability author runs `traverse-cli capability publish` (in the `traverse` repo), which validates the contract locally and opens a PR here automatically.
2. CI runs deterministic checks (schema, semver-bump-vs-diff, digest integrity, namespace collisions, dependency resolvability). The advisory AI pass (duplicate/boundary-quality flags) runs in-chat via the `capability-review` skill (`.agents/skills/capability-review/`) during the owner's review — the CI job for it is intentionally dormant (no API key; see `docs/decision-log.md` entries 19 and 25) and posts a degraded-mode notice.
3. A human reviews and approves — automated checks alone can never merge a publish, and the advisory pass never blocks one.
4. Merging to `main` builds a versioned index artifact and publishes it as a GitHub Release.
5. Anyone running `traverse-cli registry sync` fetches that release into local workspace state — the runtime never talks to this repo live.

Building with an AI coding agent? [traverse-framework/claude-skills](https://github.com/traverse-framework/claude-skills) hosts a Claude Skill that checks this registry before authoring a new capability, so you don't duplicate something that's already published.

## Layout

```text
capabilities/<namespace>/<id>/<version>/contract.json   # published capability records
specs/                                                   # this repo's own governing specs
docs/                                                    # decision log and supporting docs
.specify/                                                # spec-driven workflow scaffold
scripts/ci/                                              # CI gate scripts (spec-alignment, vendored from traverse-framework/.github)
```

## Governance

This repo follows the shared governance model defined in [`traverse-framework/.github`](https://github.com/traverse-framework/.github). See `.specify/memory/constitution.md` for the pinned version this repo has adopted, and `CONTRIBUTING.md` before opening a PR.
