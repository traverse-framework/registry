---
name: ship
description: "Take one named issue in traverse-framework/registry from Ready to a merged PR with green CI. Use when the user names a single issue number and says ship/finish/implement it."
---

# Ship

This is a thin pointer, not the source of truth. The actual workflow lives
at `.agents/skills/ship/SKILL.md` — the same shared-across-agent-types
location `registry-ops` uses (see `AGENTS.md`), so it is written once and
stays canonical there instead of drifting across per-tool copies.

Read `.agents/skills/ship/SKILL.md` in full and follow it exactly as
written. This file exists only so `/ship` resolves to that content as a
real Claude Code slash command.
