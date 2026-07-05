# Registry Development Guidelines

## Governance

This repo's constitution, NFRs, quality standards, antipatterns, compatibility policy, exception process, and CLA are **not** duplicated here — they live in [`traverse-framework/.github`](https://github.com/traverse-framework/.github), pinned at **governance version 1.0.0** (see `.specify/memory/constitution.md`).

Read `specs/001-registry-foundation/spec.md` before any implementation work.

## Project Structure

```text
capabilities/<namespace>/<id>/<version>/contract.json   # published capability records
specs/                                                   # this repo's own governing specs
docs/decision-log.md                                     # why this repo's design is what it is
.specify/                                                # spec-driven workflow scaffold (vendored)
scripts/ci/                                              # CI gate scripts (vendored from traverse-framework/.github)
```

## Commands

```bash
bash scripts/ci/spec_alignment_check.sh <pr-body-file>   # spec-alignment gate
python3 scripts/ci/capability_validation.py               # deterministic capability checks
python3 scripts/ci/build_index.py <prev_version> <sha> <out>  # index build
```

## Code Style

- No `unsafe`, no `unwrap()`, no `panic!()`, no TODO in code
- 100% coverage for core logic
- Deterministic: same inputs must produce same outputs

## Lean Implementation

Before adding code, apply the same minimality ladder used across `traverse-framework`:

1. Confirm the change is required by the active issue and governing spec.
2. Reuse existing code, contracts, specs, and docs when they already fit.
3. Prefer stdlib/existing dependencies over new abstractions.
4. Prefer a schema, validation branch, test, or doc update when that solves the issue.
5. Prefer one focused function or script change before adding broader structure.
6. Add only the minimum new structure needed for the issue.

Minimality must not reduce spec alignment, contract validation, stable error codes, traceability, or required tests.

<!-- MANUAL ADDITIONS START -->
## Agent Coordination

**Before starting any work on an issue**, run these pre-flight checks:

### 1. Check for a competing agent claim

```bash
gh issue view <NUMBER> --repo traverse-framework/registry --json labels
```

If the labels include `agent:claude` (and you are not Claude Code) or `agent:codex` (and you are not Codex) → **STOP**. Report:
> Issue #\<NUMBER\> is claimed by another agent. Choose a different ticket.

### 2. Check for a competing branch

```bash
git ls-remote --heads origin | grep "issue-<NUMBER>-"
```

If a branch for this issue already exists under another agent's prefix (`claude/issue-<NUMBER>-*` or `codex/issue-<NUMBER>-*`) → **STOP**. Report:
> A branch already exists for issue #\<NUMBER\>. Choose a different ticket.

### 3. Claim the ticket (only if pre-flight passes)

```bash
# Add your agent label
gh issue edit <NUMBER> --repo traverse-framework/registry --add-label "agent:claude"   # or agent:codex

# Get the Project 3 item ID
gh project item-list 3 --owner traverse-framework --format json --limit 100 \
  --jq '.items[] | select(.content.number == <NUMBER>) | .id'

# Set Status -> In Progress
# Project ID: PVT_kwDOEbiBt84BcZQJ
# Status field ID: PVTSSF_lADOEbiBt84BcZQJzhXCa-8
# "In Progress" option ID: 47fc9ee4
gh project item-edit --project-id PVT_kwDOEbiBt84BcZQJ \
  --id <ITEM_ID> \
  --field-id PVTSSF_lADOEbiBt84BcZQJzhXCa-8 \
  --single-select-option-id 47fc9ee4
```

Note: unlike `traverse-framework/traverse`'s Project 1, Project 3 has **no separate "Agent" field** — claim signaling here is label-only (`agent:claude` / `agent:codex`) plus the Status field.

### 4. Governance

Read `.specify/memory/constitution.md` before any implementation work.

- **Spec-first**: every feature needs an approved spec in `specs/` before code
- **Contract-first**: contracts are source of truth; code conforms to contracts
- **Spec-alignment gate**: CI blocks PRs that drift from `specs/governance/approved-specs.json`
- **No self-approval**: an agent must never mark a spec `approved` in `specs/governance/approved-specs.json` on its own judgment -- that requires the repo owner's explicit sign-off
- **Immutability**: a published `capabilities/<namespace>/<id>/<version>/contract.json` is never edited once merged -- fix problems via the yank/deprecation process (`specs/005-yank-deprecation`), never by editing
- **Traceability**: all work must have a GitHub issue + Project 3 item + PR
<!-- MANUAL ADDITIONS END -->
