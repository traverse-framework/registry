# Registry — Agent Instructions

Canonical agent instructions live in [CLAUDE.md](CLAUDE.md). Org-wide rules (code style, lean-implementation ladder, claim-before-you-code) live in `traverse-framework/.github` `docs/ai-agent-hardening.md` at the version recorded in `.governance-version`.

## Claim and release

Use the shared scripts, which are vendored from `traverse-framework/.github`
v1.4.0. The Project 3 ids live in `.agent-claim.json`.

```bash
scripts/agent/claim.sh <issue> <claude|codex|cursor> --dry-run   # check only
scripts/agent/claim.sh <issue> <claude|codex|cursor>             # claim
scripts/agent/release.sh <issue> <Ready|Blocked|Done>            # release
```

`claim.sh` changes nothing unless the issue is open, its Project 3 status is
Ready, it carries no `agent:*` label, and no `*/issue-<N>-*` branch or open PR
from any tool exists. If it refuses, pick another ticket.
