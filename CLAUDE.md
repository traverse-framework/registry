# Registry Development Guidelines

## Governance

This repo's constitution, NFRs, quality standards, antipatterns, compatibility policy, exception process, and CLA are **not** duplicated here — they live in [`traverse-framework/.github`](https://github.com/traverse-framework/.github), pinned at **governance version 1.0.0** (see `.specify/memory/constitution.md`).

Read `specs/001-registry-foundation/spec.md` before any implementation work — it's this repo's own foundational spec, and it explicitly inherits semver/immutability rules from `traverse-framework/traverse`'s specs 005/037/043 rather than redefining them.

## Project Structure

```text
capabilities/<namespace>/<id>/<version>/contract.json   # published capability records (empty until first publish)
specs/                                                   # this repo's own governing specs
docs/decision-log.md                                     # why this repo's design is what it is
.specify/                                                # spec-driven workflow scaffold (vendored from traverse)
scripts/ci/                                              # CI gate scripts (vendored from traverse-framework/.github)
```

## Commands

```bash
bash scripts/ci/spec_alignment_check.sh <pr-body-file>   # spec-alignment gate (requires BASE_SHA/HEAD_SHA env)
```

No build/test commands yet — this repo has no executable code until the `traverse-registry` crate extraction (`traverse` spec 051) lands.

## Code Style

Inherited from `traverse-framework/.github`'s `docs/ai-agent-hardening.md`: no `unsafe`, no `unwrap()`, no `panic!()`, no TODO in code; 100% coverage for core logic; deterministic behavior.

## Key Rules Specific To This Repo

1. `capabilities/<namespace>/<id>/<version>/contract.json` is immutable once merged — never edit an existing version, only add new ones.
2. WASM/source artifacts are never committed directly — reference by digest + GitHub Release URL.
3. Publishing is PR-only; CI's deterministic checks + advisory AI review both run, but only human approval actually gates merge.
4. Deprecation is additive (a yank record), never an edit or deletion of the original contract.
