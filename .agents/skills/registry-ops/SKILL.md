---
name: "registry-ops"
description: "Start or resume the standard traverse-framework/registry operating model when the user says REGISTRY OPS, asks to start registry ops/dev work, asks for the ready-ticket worker, PR finisher, or backlog gardener, or wants an agent to pick ready Project 3 work and run the registry coordination process."
---

# Registry Ops

Use this skill when the user wants an agent to start or resume the standard operating model for `traverse-framework/registry`.

Canonical trigger:

```text
REGISTRY OPS
```

## Workflow

1. Read `.specify/memory/constitution.md` (a pointer to `traverse-framework/.github`, governance version 1.0.0) before implementation work.
2. Read `AGENTS.md` and follow the agent coordination rules.
3. Inspect current GitHub and Project 3 state.
4. Prefer finishing existing open PRs before claiming new Ready work.
5. If no active PR needs attention, pick one Ready Project 3 issue.
6. Before work on an issue, run the pre-flight checks from `AGENTS.md`:
   - issue must not carry another agent's label (`agent:claude` / `agent:codex`)
   - no remote branch may exist for this issue under another agent's prefix
7. If pre-flight passes, claim the issue:
   - add your agent label (`agent:claude` or `agent:codex`)
   - set Project 3 `Status` to `In Progress` (Project 3 has no separate `Agent` field -- unlike `traverse`'s Project 1 -- so the label alone signals ownership)
8. Use a dedicated `<agent>/issue-NNN-*` branch (e.g. `claude/issue-12-*`).
9. Keep work scoped to the claimed issue and governing spec.
10. Open a dedicated PR with validation evidence, declaring the governing spec in `## Governing Spec`.

## Registry-Specific Rules

- **No self-approval of specs**: never move a spec from `Draft` to `Approved` in `specs/governance/approved-specs.json` on your own judgment. That requires the repo owner's explicit, standalone sign-off -- not a bundled "ok" answering an unrelated question. This mirrors the constitution's Principle II (no publication by automation alone).
- **Immutability is structural, not just a rule**: a merged `capabilities/<namespace>/<id>/<version>/contract.json` is never edited. A yank is an additive `deprecated.json` sibling file (see `specs/005-yank-deprecation`), never a modification.
- **Publishing is PR-only**: capability publishing happens by `traverse-cli capability publish` (in the `traverse` repo) opening a PR here, or by a manually-opened PR following the same shape. Deterministic CI checks (`capability_validation.py`) plus an advisory AI pass gate it; only a human merge is final.
- **Cross-repo actions need explicit, standalone confirmation**: repo renames, deleting/disabling org-level Project automations, and crate-extraction work spanning `traverse` + `registry` are the kind of action that must not proceed on an ambiguous or bundled "ok" -- ask for (or wait for) a direct, unambiguous instruction naming the action.

## Token Discipline

Same lean-by-default style as `traverse-ops`:

- Prefer targeted GitHub queries over full board dumps: `gh project item-list 3 --owner traverse-framework --format json --limit 100 --jq '...'`, returning only issue number, title, labels, and item id.
- Do not paste full `gh project item-list`, `gh pr checks --watch`, test, or CI logs into the conversation. Summarize pass/fail and quote only the failing lines needed to fix the issue.
- Use `git diff --stat` / `git diff --name-only` before large diffs.
- Keep progress updates short: current action, discovered blocker if any, next action.
- After CI starts, poll with bounded output; on failure, fetch only that job's log and extract the actionable failure.
- Prefer local reproduction of a failing gate (e.g. `python3 scripts/ci/capability_validation.py`) before fetching remote logs.

## Minimality Ladder

Before adding code, apply this registry-specific minimality ladder:

1. Does this change need to exist for the active issue and governing spec?
2. Can existing registry content (specs, decision log, scripts) already satisfy it?
3. Can an existing script, dependency, or CI job do it with a small extension?
4. Can a schema field, validation branch, test, or doc update solve it without a new abstraction?
5. Can one focused function or script change solve it?
6. Only then add the minimum new structure needed for the issue.

Minimality must never weaken spec alignment, contract immutability, digest verification, dependency resolvability, or required tests. Create follow-up tickets for useful adjacent improvements instead of expanding an active slice.

## Operating Lanes

- **Ready-ticket worker**: claim one Ready Project 3 issue and implement it end to end.
- **PR finisher**: inspect open PRs, fix CI/review issues, update stale branches, and merge when green if allowed.
- **Backlog gardener**: audit Project 3 statuses, labels, blockers, and missing tickets -- including checking for stray items swept in by the org's "Auto-add to project" automation (a known, recurring issue as of 2026-07) and removing any whose `repository` field isn't `traverse-framework/registry`.

## Guardrails

- Do not mark work `In Progress` unless a real dev thread has started it.
- Do not use labels as status; Project 3 `Status` is the actionability source of truth.
- Do not claim work already owned by another agent.
- Do not broaden scope beyond the issue and governing spec.
- Do not approve a spec, execute a repo rename, or touch org-level Project automation settings without explicit, standalone confirmation (see Registry-Specific Rules above).
- Create future tickets for non-blocking improvements instead of expanding an active slice.

For the full narrative reasoning behind this repo's design, see `docs/decision-log.md`.
