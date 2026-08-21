# Contributing

Thanks for contributing to the Traverse Registry.

## Before You Start

Please read:

- [specs/001-registry-foundation/spec.md](specs/001-registry-foundation/spec.md) — this repo's foundational spec
- [traverse-framework/.github](https://github.com/traverse-framework/.github) — constitution, quality standards, antipatterns, compatibility policy, exception process, CLA (this repo has adopted governance version 1.0.0)
- [docs/decision-log.md](docs/decision-log.md) — why this repo's design is what it is

## Publishing a Capability

Use `traverse-cli capability publish` (from the `traverse` repo) rather than hand-crafting a PR — it validates your contract locally and opens the PR for you. See `specs/001-registry-foundation/spec.md`, User Story 1.

`traverse-cli`'s local validation can lag this repo's own CI gates — new requirements
(e.g. `specs/017-persona-registry`, the FR-020 capability inventory) land here first and
the CLI can report "passed" on a PR that CI then rejects (see `docs/decision-log.md`
entry 61). **Before opening the PR**, reproduce CI's two required checks locally — same
scripts CI runs, so a clean pass here means no CI round-trip:

```bash
bash scripts/ci/pre_pr_check.sh <path-to-draft-pr-body.md>
```

The file you pass must contain your draft PR description, including its
`## Governing Spec` section — every changed file's governing spec(s) (per
`specs/governance/approved-specs.json`) must be declared there, or `spec-alignment` fails.
If you're adding a new `personas/<id>/<version>/persona.json`, scaffold it with
`scripts/scaffold/new-persona.sh` rather than hand-writing it — it prompts for every
required field (including `distinguished_from`) and self-validates before you commit.

Publishing more than one capability in the same session? Branch each
`publish/<capability>-<version>` branch from `origin/main` — never from another in-flight
`publish/*` branch. This repo squash-merges PRs, which severs shared history between a
merged branch and anything built on top of it; a branch stacked on another `publish/*`
branch becomes unmergeable (or silently redundant) the moment the earlier one merges, with
no warning until then. See `docs/decision-log.md` entry 62 for what this looked like in
practice (`#280`/`#281`).

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
