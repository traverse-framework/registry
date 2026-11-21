# Registry Development Guidelines

## Governance

This repo's constitution, NFRs, quality standards, antipatterns, compatibility policy, exception process, and CLA are **not** duplicated here — they live in [`traverse-framework/.github`](https://github.com/traverse-framework/.github), pinned at the version recorded in `.governance-version`.

Read `specs/001-registry-foundation/spec.md` before any implementation work — it's this repo's own foundational spec, and it explicitly inherits semver/immutability rules from `traverse-framework/traverse`'s specs 005/037/043 rather than redefining them.

## Project Structure

```text
capabilities/<namespace>/<id>/<version>/contract.json   # published capability records
capability-src/<name>/                                   # source for each published capability's WASM artifact (no_std Rust)
capability-src/wasi-capability-runtime/                  # shared no_std runtime shim used by every capability crate
specs/                                                   # this repo's own governing specs
docs/decision-log.md                                     # why this repo's design is what it is
.specify/                                                # spec-driven workflow scaffold (vendored from traverse)
scripts/ci/                                              # CI gate scripts (vendored from traverse-framework/.github)
```

"Agent" is reserved for a capability that genuinely uses model-backed reasoning; every
capability here today is deterministic business logic (decision-log entry 41).

## Commands

```bash
bash scripts/ci/spec_alignment_check.sh <pr-body-file>   # spec-alignment gate (requires BASE_SHA/HEAD_SHA env)
python3 scripts/ci/capability_validation.py               # deterministic capability checks (whole-tree only; pass BASE_SHA HEAD_SHA for the diff-based checks CI also runs, e.g. persona_ref, immutability — see pre_pr_check.sh below)
bash scripts/ci/pre_pr_check.sh <pr-body-file>            # both gates above, wired exactly as CI wires them — the actual pre-PR command to run
python3 scripts/ci/build_index.py <prev_version> <sha> <out>  # index build
(cd capability-src/<name> && cargo test)                  # unit-tests one capability's logic (host target)
cargo test -p traverse-registry --locked                  # crate unit + integration tests
cargo clippy -p traverse-registry --all-targets --locked -- -D warnings  # crate lint gate (matches CI exactly)
```

Before pushing a `crates/traverse-registry` change, run its `cargo test` and `cargo clippy`
lines above; they are exactly what CI runs.

Before any PR touching `capabilities/` or `personas/`, run `scripts/ci/pre_pr_check.sh`.
A bare `capability_validation.py` silently skips the diff-based rules (`persona_ref`,
immutability, scenario format), which is how `#278` failed CI (decision-log entry 61).
Scaffold new `personas/` records with `scripts/scaffold/new-persona.sh`.

## Git & PR Workflow

- **Check existing state before starting a ticket** (`gh issue view <n>`, `gh pr list --search "<n>" --state all`). A closed issue beats a stale Project 3 `Status`.
- **Every PR body needs `## Governing Spec`** with bare `- <spec-id>` bullets whose `governs` prefix in `specs/governance/approved-specs.json` matches a changed file (required `spec-alignment` check). `capability-src/` needs `018-capability-test-coverage` (plus `024`/`027` for `catalog-builder/`, `model-execute/`); for ungoverned files like `CLAUDE.md` or `.agents/`, any approved spec such as `001-registry-foundation` works.
- `spec-alignment` reads the body from the triggering push, so after editing the body push a new commit (`--allow-empty` is fine).
- No pre-push hook here; a plain `git push -u origin <branch>` works.

## Code Style

Inherited from `traverse-framework/.github`'s `docs/ai-agent-hardening.md`: no `unsafe`, no `unwrap()`, no `panic!()`, no TODO in code; 100% coverage for core logic; deterministic behavior.

## Key Rules Specific To This Repo

1. `capabilities/<namespace>/<id>/<version>/contract.json` is immutable once merged — never edit an existing version, only add new ones.
2. WASM/source artifacts are never committed directly — reference by digest + GitHub Release URL.
3. Publishing is PR-only; CI runs the deterministic checks, the advisory AI review runs in-chat via the `capability-review` skill (`.agents/skills/capability-review/` — CI's version is intentionally dormant, decision-log entries 19/25), and only human approval actually gates merge.
4. Deprecation is additive (a yank record), never an edit or deletion of the original contract.

## Working Style

- Unattended, don't ask about reversible low-stakes choices: pick the sensible default and note it in the PR. Ask only for irreversible actions, public API/security changes, or real ambiguity (the registry-ops skill lists the hard stops).
- Batch independent read-only commands into one shell call.
- If the request contradicts the code, specs or a live check, surface the evidence (file:line, output) before implementing; if it stays unresolved, record it in `docs/decision-log.md` or an issue.
- For audits, fan out per subsystem, then verify every finding against source before reporting it. Keep shipping work (PRs, merges) serial.
