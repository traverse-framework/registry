---
name: registry-ops
description: "Start or resume the standard traverse-framework/registry operating model when the user says REGISTRY OPS, asks to start registry ops/dev work, asks for the ready-ticket worker, PR finisher, or backlog gardener, or wants an agent to pick ready Project 3 work and run the registry coordination process."
---

# Registry Ops

This is a thin pointer, not the source of truth. The actual workflow lives at
`.agents/skills/registry-ops/SKILL.md` — a location deliberately shared
across every agent type this repo coordinates (Claude Code, Codex, etc; see
`AGENTS.md` and PR #16), so it is written once and stays canonical there
instead of drifting across per-tool copies.

Read `.agents/skills/registry-ops/SKILL.md` in full and follow it exactly as
written. This file exists only so `/registry-ops` resolves to that content
as a real Claude Code slash command.

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
