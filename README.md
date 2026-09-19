# Traverse Registry

The public capability registry for [Traverse](https://github.com/traverse-framework/traverse) — a git-based, CI-validated, PR-published registry of capability contracts and their artifacts.

## What This Is

Traverse's runtime resolves capabilities through `traverse-registry` (a crate that lives here — see `specs/051-registry-extraction/spec.md` in the `traverse` repo for the migration record). That crate needs somewhere real to resolve capabilities *from*. This repo is that place: capability records live here as reviewed, versioned files; CI enforces quality and immutability; publishing a capability means opening a PR.

Read [`specs/001-registry-foundation/spec.md`](specs/001-registry-foundation/spec.md) for the full governing spec, and [`docs/decision-log.md`](docs/decision-log.md) for the reasoning behind every major decision.

## How Publishing Works

**Usual authoring path (no Rust required from you):** the Claude skill
[`traverse-capability-author`](https://github.com/traverse-framework/claude-skills/tree/main/skills/traverse-capability-author)
interviews you in plain English, checks this registry (and any private one),
writes the contract, produces executable WASM, and opens a human-reviewed PR
here. Under the hood that is still Rust→WASM. Manual authoring still uses
Rust→WASM and ends in the same publish flow.

1. A capability author runs `traverse-cli capability publish` (in the `traverse` repo), which validates the contract locally and opens a PR here automatically.
2. CI runs deterministic checks (schema, semver-bump-vs-diff, digest integrity, namespace collisions, dependency resolvability). The advisory AI pass (duplicate/boundary-quality flags) runs in-chat via the `capability-review` skill (`.agents/skills/capability-review/`) during the owner's review — the CI job for it is intentionally dormant (no API key; see `docs/decision-log.md` entries 19 and 25) and posts a degraded-mode notice.
3. A human reviews and approves — automated checks alone can never merge a publish, and the advisory pass never blocks one.
4. Merging to `main` builds a versioned index artifact and publishes it as a GitHub Release.
5. Anyone running `traverse-cli registry sync` fetches that release into local workspace state — the runtime never talks to this repo live.

### Artifact references on new publishes

Newly **added** `capabilities/**/contract.json` files must include `artifact.digest` (`sha256:…`) and `artifact.url` pointing at a GitHub Release asset under `https://github.com/traverse-framework/registry/releases/download/artifacts/<tag>/<asset>` (see `specs/007-artifact-hosting/spec.md`). CI rejects missing or non-matching references at PR time so unusable records never reach the index. Until [traverse#859](https://github.com/traverse-framework/traverse/issues/859) is fixed, verify `traverse-cli capability publish` did not strip these fields from the opened PR.

Prefer starting from [traverse-framework/claude-skills](https://github.com/traverse-framework/claude-skills) (`traverse-capability-author`) so the registry is checked before anything new is drafted. Manual Rust→WASM remains available; both paths publish through the same human-reviewed PR gates here.

## Layout

```text
capabilities/<namespace>/<id>/<version>/contract.json   # published capability records
specs/                                                   # this repo's own governing specs
docs/                                                    # decision log and supporting docs
.specify/                                                # spec-driven workflow scaffold
scripts/ci/                                              # CI gate scripts (spec-alignment, vendored from traverse-framework/.github)
```

## Governance

This repo follows the shared governance model defined in [`traverse-framework/.github`](https://github.com/traverse-framework/.github). See `.specify/memory/constitution.md` for the pinned version this repo has adopted, and [`CONTRIBUTING.md`](CONTRIBUTING.md) before opening a PR.

**First time publishing?** Grab a [`help wanted` / `good first issue`](https://github.com/traverse-framework/registry/issues?q=is%3Aissue+is%3Aopen+label%3A%22help+wanted%22+label%3A%22good+first+issue%22) ticket (see [First contributions](CONTRIBUTING.md#first-contributions)) — one capability per PR, through the normal gates.
