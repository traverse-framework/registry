---
name: "ship"
description: "Take one named issue in traverse-framework/registry from Ready to a merged PR with green CI -- claim, implement, test, open a correctly-formatted PR, queue merge, verify. Use when the user names a single issue number and says ship/finish/implement it. Use REGISTRY OPS instead when they want the continuous backlog loop across many tickets."
---

# Ship

Single-ticket version of the registry-ops operating model: take one named
issue from claim to merged PR, then stop. Use `REGISTRY OPS`
(`.agents/skills/registry-ops/SKILL.md`) instead when the user wants the
continuous backlog loop across many tickets — this skill exists for the
"just do #<n>" case, so it doesn't loop back to claim more work afterward.

## Workflow

1. **Check existing state first.** `gh issue view <n>`, `gh pr list --search "<n>" --state all`, and the Project 3 board status field. If a PR already exists or the issue is already closed, report that and stop — do not duplicate work. Project 3's `Status` field can lag a merged/closed issue; a closed GitHub issue is more authoritative than a stale board column.
2. **Pre-flight before claiming**: the issue must not carry another agent's label (`agent:claude` / `agent:codex` / `agent:cursor`), and no remote branch may exist for it under another agent's prefix. If either is true, stop and report — do not claim work already owned by another agent.
3. **Claim**: add your agent label to the issue, set its Project 3 `Status` to `In Progress`.
4. Branch: `claude/issue-<n>-<slug>` off latest `origin/main`.
5. Implement, scoped to the issue and its governing spec. If unsure how much to build, apply the Minimality Ladder in `.agents/skills/registry-ops/SKILL.md`.
6. **Before pushing**, run and fix failures — this mirrors CI exactly, so a clean local pass means no CI round-trip:
   ```bash
   cargo test -p traverse-registry --locked
   cargo clippy -p traverse-registry --all-targets --locked -- -D warnings
   python3 scripts/ci/capability_validation.py   # only if capabilities/ changed
   ```
7. Open a PR using the org body superset: `## Summary`, `## Governing Spec`, `## Project Item`, `## Definition of Done`, `## Validation`. The `## Governing Spec` section must list every approved spec ID whose `governs` prefix in `specs/governance/approved-specs.json` matches at least one changed file — cross-check that file directly, don't guess. `capability-src/` and `CLAUDE.md` currently aren't covered by any spec's `governs` list; cite `001-registry-foundation` there per existing precedent.
8. Queue merge: `gh pr merge <n> --squash --auto`.
9. **Verify, don't assume.** Poll `gh pr checks <n>` until nothing is `pending`, then confirm `gh pr view <n> --json state,mergedAt` actually shows `MERGED` — a queued PR can sit `BLOCKED` forever on a real failing required check. `spec-alignment` specifically reads the PR body from the triggering push/sync event, so `gh pr edit` alone does not re-run it after a body fix; push a new commit (`git commit --allow-empty` is fine) instead.
10. Check whether Project 3 status auto-updated to `Done`; board sync on merge sometimes lags a closed issue, so set it manually if it didn't.

## Guardrails

Same hard stops as `REGISTRY OPS`: no self-approval of specs, no unrequested cross-repo actions, don't claim work already owned by another agent, don't broaden scope beyond the issue and its governing spec. See `.agents/skills/registry-ops/SKILL.md`'s own Registry-Specific Rules and Guardrails sections for the full list — this skill inherits them, it doesn't restate them.
