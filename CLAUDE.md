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

`capability-src/` was named `agents/` until 2026-07-29 (decision-log entry 41) — "agent" is
now a reserved term for a future capability whose implementation genuinely involves
AI/model-backed reasoning; every capability published in this registry today is pure,
deterministic business logic, so none of them qualify.

## Commands

```bash
bash scripts/ci/spec_alignment_check.sh <pr-body-file>   # spec-alignment gate (requires BASE_SHA/HEAD_SHA env)
python3 scripts/ci/capability_validation.py               # deterministic capability checks
python3 scripts/ci/build_index.py <prev_version> <sha> <out>  # index build
(cd capability-src/<name> && cargo test)                  # unit-tests one capability's logic (host target)
cargo test -p traverse-registry --locked                  # crate unit + integration tests
cargo clippy -p traverse-registry --all-targets --locked -- -D warnings  # crate lint gate (matches CI exactly)
```

The `traverse-registry` crate extraction (`traverse` spec 051) has landed — see
`docs/decision-log.md` entries 26/29/30 — but every capability under `capability-src/`
already had real, tested, buildable Rust source before and independent of that; this repo
has not been executable-code-free since decision-log entry 38.

### Before pushing any `crates/traverse-registry` change

Run, in order, and fix failures before pushing — this is exactly what CI runs, so a clean
local pass means no CI round-trip:

```bash
cargo test -p traverse-registry --locked
cargo clippy -p traverse-registry --all-targets --locked -- -D warnings
python3 scripts/ci/capability_validation.py   # only if capabilities/ changed
```

## Git & PR Workflow

- **Check existing state before starting any ticket.** Run `gh issue view <n>`, `gh pr list --search "<n>"` (or `--search "<n>" --state all`), and check the Project 3 board status field before writing any code. If a PR already exists or the issue is already closed, report that instead of duplicating work — Project 3's `Status` field is sometimes stale (doesn't always auto-sync on merge), so a closed GitHub issue is more authoritative than a board column that still says `In Progress`.
- **Every PR body must include a `## Governing Spec` section** listing the approved spec ID(s) (bare `- <spec-id>` bullets, e.g. `- 037-semver-range-resolution`) whose `governs` prefix in `specs/governance/approved-specs.json` matches at least one changed file. This isn't optional style — `spec-alignment` is a required CI check and fails the PR without it. Cross-check `specs/governance/approved-specs.json` directly rather than guessing; `capability-src/` currently isn't covered by any spec's `governs` list, so cite `001-registry-foundation` there per existing precedent.
- **`spec-alignment` reads the PR body from the triggering push/sync event**, not the live PR state — editing the body with `gh pr edit` alone does not re-run the check. Push a new commit (`git commit --allow-empty` is fine) after fixing the body to force a fresh event.
- This repo has no pre-push git hook — pushing a brand-new branch works normally with a plain `git push -u origin <branch>`.

## Code Style

Inherited from `traverse-framework/.github`'s `docs/ai-agent-hardening.md`: no `unsafe`, no `unwrap()`, no `panic!()`, no TODO in code; 100% coverage for core logic; deterministic behavior.

## Key Rules Specific To This Repo

1. `capabilities/<namespace>/<id>/<version>/contract.json` is immutable once merged — never edit an existing version, only add new ones.
2. WASM/source artifacts are never committed directly — reference by digest + GitHub Release URL.
3. Publishing is PR-only; CI runs the deterministic checks, the advisory AI review runs in-chat via the `capability-review` skill (`.agents/skills/capability-review/` — CI's version is intentionally dormant, decision-log entries 19/25), and only human approval actually gates merge.
4. Deprecation is additive (a yank record), never an edit or deletion of the original contract.

## Working Style

When operating autonomously (registry-ops loops, unattended ticket work), do not stop to ask for reversible, low-stakes choices — pick the sensible default, proceed, and note the assumption in the PR description if it's non-obvious. Reserve actual questions for irreversible actions, public API/security-posture changes, or genuine ambiguity a human must resolve (see the registry-ops skill's own guardrails on spec approval and cross-repo actions for the hard stops that always apply).
