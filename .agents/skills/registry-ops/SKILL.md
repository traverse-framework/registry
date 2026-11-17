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

**This is a continuous operating loop, not a single-ticket task.** Invoking
this skill means: keep working -- across as many issues and PRs as it takes
-- until `traverse-framework/registry`'s Project 3 has no `Ready` or `In
Progress` items left and every PR opened this run has actually merged
(verified via `gh pr view --json state,mergeStateStatus`, not just that
auto-merge was enabled -- a queued PR can sit `BLOCKED` forever on a real
failing gate). Do not stop after one issue or one PR to ask whether to
continue; only stop early for a real guardrail (see Registry-Specific
Rules) or genuine blockage (e.g. every remaining `Ready` item is an
umbrella with no actionable sub-scope, or the next step requires a decision
only the repo owner can make).

1. Read `AGENTS.md` and follow the agent coordination rules.
2. Read the constitution (via `traverse-framework/.github`, at the version in `.governance-version` — never a hardcoded number) only when the ticket touches architecture, contracts, or versioned surfaces; lazy-read map in the org's `docs/ai-agent-hardening.md`.
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
10. Open a dedicated PR using the org body superset (`## Summary`, `## Governing Spec`, `## Project Item`, `## Definition of Done`, `## Validation`), declaring **every** approved spec whose `governs` prefix matches a changed file (not just the one most relevant to the PR's narrative -- cross-check `specs/governance/approved-specs.json` directly), then for full-CI PRs follow **Merge Gate** (add `needs-review`, wait for a passing review record) before squash-merge. Do not poll checks in a tight loop -- move on to the next issue -- but before starting the *next* claim, or at the very start of a resumed pass, check every PR opened this run (`gh pr checks <N>`; `gh pr view <N> --json state,mergeStateStatus`). A `BLOCKED` merge state on a real failing required check (`spec-alignment` and `capability-validation` are both required as of the #39 fix, 2026-07-29) will never resolve on its own -- fix it (see Gates & Failure Playbook) and re-push rather than leaving it queued and assuming auto-merge will sort it out.
11. Loop: go back to step 3. Keep claiming the next open PR needing attention or next Ready issue until none remain and every opened PR is confirmed `MERGED`.

## Registry-Specific Rules

- **No self-approval of specs**: never move a spec from `Draft` to `Approved` in `specs/governance/approved-specs.json` on your own judgment. That requires the repo owner's explicit, standalone sign-off -- not a bundled "ok" answering an unrelated question. This mirrors the constitution's Principle II (no publication by automation alone).
- **Immutability is structural, not just a rule**: a merged `capabilities/<namespace>/<id>/<version>/contract.json` is never edited. A yank is an additive `deprecated.json` sibling file (see `specs/005-yank-deprecation`), never a modification.
- **Publishing is PR-only**: capability publishing happens by `traverse-cli capability publish` (in the `traverse` repo) opening a PR here, or by a manually-opened PR following the same shape. Deterministic CI checks (`capability_validation.py`) plus an advisory AI pass gate it; only a human merge is final.
- **Cross-repo actions need explicit, standalone confirmation**: repo renames, deleting/disabling org-level Project automations, and crate-extraction work spanning `traverse` + `registry` are the kind of action that must not proceed on an ambiguous or bundled "ok" -- ask for (or wait for) a direct, unambiguous instruction naming the action.

## Gates & Failure Playbook

Every PR must pass the org gates `cla / cla` and `baseline / governance-baseline` plus this repo's CI (`spec-alignment` and `capability-validation` are branch-protection-required as of the #39 fix -- a red result genuinely blocks merge, it does not just advise). When a governance gate fails, use the failure playbook in `traverse-framework/.github` `docs/runbook.md` (CLA `recheck` comment; re-runs pin stale gate snapshots, push a commit instead; secret-visibility check). Dependabot PRs: comment `@dependabot rebase`, queue `gh pr merge --squash --auto`, and let CI decide — never hand-write their bodies.

**`spec-alignment` failures specifically**: the workflow reads `github.event.pull_request.body` from the triggering push/sync event, so editing the PR body alone (`gh pr edit`) does not re-run the check -- fix the body, then push a new commit (`git commit --allow-empty` is fine if there's nothing else to change) to force a fresh event with the corrected body.

## Token Discipline

Org-canon token rules live in `traverse-framework/.github` `docs/ai-agent-hardening.md`
(pinned via `.governance-version`): bounded `--limit` queries with server-side `--jq`,
no raw board/CI/test log dumps, targeted diffs, short progress updates.
Registry-specific addition:

- Prefer local reproduction of a failing gate (`python3 scripts/ci/capability_validation.py`,
  `bash scripts/ci/spec_alignment_check.sh`) before fetching remote logs.

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

These three lanes are not alternatives to pick once -- a full registry-ops
pass cycles through all of them repeatedly (PR finisher, then ready-ticket
worker, then back to PR finisher for what was just opened) until the loop
condition in Workflow is met: no Ready/In Progress items, no unmerged PRs.

- **Ready-ticket worker**: claim one Ready Project 3 issue and implement it end to end, then return to the loop for the next one.
- **PR finisher**: inspect open PRs, fix CI/review issues, update stale branches, and merge when green if allowed.
- **Review queue**: drain `needs-review` PRs from other tools per Decision 133.
- **Backlog gardener**: audit Project 3 statuses, labels, blockers, and missing tickets -- including checking for stray items swept in by the org's "Auto-add to project" automation (a known, recurring issue as of 2026-07) and removing any whose `repository` field isn't `traverse-framework/registry`.

## Review Queue (Decision 133)

Before claiming new Ready work, drain the cross-tool review queue:

1. List open PRs labelled `needs-review` whose head branch prefix is a **different**
   tool than this session (`claude/`, `codex/`, or `cursor/`).
2. Claim one (comment that this tool is reviewing; do not review your own prefix
   except via the 4-hour fallback below).
3. In a **fresh** context, review the PR head against the rubric in
   `traverse-framework/.github` `docs/ai-agent-hardening.md` (governing-spec match,
   explicit failure modes, no weakened gates, minimality, behavior-asserting tests).
4. Post a single PR comment whose body starts with the HTML review-record header
   (`tool`, `commit` = current head SHA, `verdict` = `pass` or `changes-requested`,
   `fallback`).
5. On `changes-requested`: wait for the author to fix or rebut, then re-check the
   new head. After one unresolved round, add `needs-enrico`.

Same-tool fallback: only if `needs-review` has been on the PR for ≥ 4 hours, and
only in a fresh session. Record `fallback: true`. Never review your own branch
prefix without that recorded fallback.

## Merge Gate (Decision 133)

For a PR this tool authored that the change classifier sends to full CI:

1. When CI is green, add the `needs-review` label if it is missing.
2. Do **not** merge until a passing review record exists for the current head
   commit from a different tool (or a recorded same-tool fallback after 4 hours).
3. In governance 1.3.x the CI check warns when the record is missing; still treat
   the record as required for merge in this skill.
4. Dependabot and version-only release PRs are exempt; docs-only PRs are exempt
   via `pr_change_classification.sh`.
5. Then squash-merge, set Project status to Done, and continue the drain loop.

Never use `--no-verify` or redirect `core.hooksPath`. First push: write the PR
body file and push with `TRAVERSE_PR_BODY=/path/to/body.md git push -u origin HEAD`
(or `.git/traverse-pr-body.md`). When a new failure costs a full CI round-trip,
add a symptom/cause/fix entry to `docs/agent-pitfalls.md` in the hardening PR.

## Guardrails

- Do not mark work `In Progress` unless a real dev thread has started it.
- Do not use labels as status; Project 3 `Status` is the actionability source of truth.
- Do not claim work already owned by another agent.
- Do not broaden scope beyond the issue and governing spec.
- Do not approve a spec, execute a repo rename, or touch org-level Project automation settings without explicit, standalone confirmation (see Registry-Specific Rules above).
- Create future tickets for non-blocking improvements instead of expanding an active slice.

For the full narrative reasoning behind this repo's design, see `docs/decision-log.md`.
