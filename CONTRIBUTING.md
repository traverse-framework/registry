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

### Test coverage is mandatory, for every publisher

Every new capability — whether you work in this repo's usual group or are an outside
team publishing for the first time — **must include real Rust source with measured test
coverage in this same PR**. A contract.json plus a compiled `.wasm` artifact is not
enough (`specs/018-capability-test-coverage`, `docs/decision-log.md` entry 64):

1. Add your implementation at `capability-src/<capability-id-with-every-"."-replaced-by-"-">/`
   — e.g. `artifact.revision-create` → `capability-src/artifact-revision-create/`. This
   exact naming rule is what CI checks for; there is no separate registration step.
2. Write real `#[test]` cases covering every branch of your own logic (not the WASI I/O
   harness — that's `#[cfg(not(test))]` and excluded from what's measured).
3. Before opening the PR, check locally:
   ```bash
   rustup component add llvm-tools-preview
   cargo install cargo-llvm-cov   # once, if you don't already have it
   cargo llvm-cov --summary-only --json --manifest-path capability-src/<your-crate>/Cargo.toml
   ```
   CI requires `functions.percent == 100.0` and `lines.percent >= 95.0` and
   `regions.percent >= 95.0` from that same command's output. A small allowance below
   literal 100% on lines/regions exists because genuinely unreachable defensive branches
   (e.g. an `unwrap_or` fallback a prior validation already rules out) are real in Rust —
   functions have no such exception: every function you ship must be exercised by a test.

There is no attestation-only path — CI runs your test suite itself rather than trusting a
self-reported percentage, the same reason `docs/decision-log.md` entry 61 stopped trusting
`traverse-cli`'s own local-validation claim. If your organization can't contribute source
into this repo, your capability cannot be published here; publish elsewhere and reference
it, rather than opening a contract-only PR that CI will reject.

Publishing more than one capability in the same session? Branch each
`publish/<capability>-<version>` branch from `origin/main` — never from another in-flight
`publish/*` branch. This repo squash-merges PRs, which severs shared history between a
merged branch and anything built on top of it; a branch stacked on another `publish/*`
branch becomes unmergeable (or silently redundant) the moment the earlier one merges. See
`docs/decision-log.md` entry 62 for what this looked like in practice (`#280`/`#281`), and
entry 63 for a second recurrence (`#283`/`#287`/`#288`/`#289`/`#291`/`#294`) that stayed
green and mergeable for hours after going stale. Main's branch protection now requires
`required_status_checks.strict` (branches must be up to date before merging), so a stale or
stacked branch will surface as a merge/CI conflict at update time instead of silently
sitting green — but branching correctly in the first place still avoids the rebuild.

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
