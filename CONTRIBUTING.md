# Contributing

Thanks for contributing to the Traverse Registry.

## Before You Start

Please read:

- [specs/001-registry-foundation/spec.md](specs/001-registry-foundation/spec.md) — this repo's foundational spec
- [traverse-framework/.github](https://github.com/traverse-framework/.github) — constitution, quality standards, antipatterns, compatibility policy, exception process, CLA (this repo has adopted governance version 1.0.0)
- [docs/decision-log.md](docs/decision-log.md) — why this repo's design is what it is

## Publishing a Capability

Use `traverse-cli capability publish` (from the `traverse` repo) rather than hand-crafting a PR — it validates your contract locally and opens the PR for you. See `specs/001-registry-foundation/spec.md`, User Story 1.

## Core Rules

- Approved specs are versioned, immutable, and merge-gating.
- A published `capabilities/<namespace>/<id>/<version>/contract.json` is immutable — fix problems via the yank/deprecation process, never by editing.
- All contributions are governed by the CLA at `traverse-framework/.github/CLA.md`.

## Pull Requests

Every pull request should:

- reference the governing spec version in a `## Governing Spec` section
- reference the relevant issue or Project item
- explain any compatibility impact

Pull requests should not merge if:

- deterministic CI checks fail
- a required CLA has not been accepted
- the change edits an already-published capability version in place instead of adding a new one
