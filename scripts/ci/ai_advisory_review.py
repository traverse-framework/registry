#!/usr/bin/env python3
"""AI-advisory PR review pass (non-blocking).

Implements specs/004-ai-advisory-review/spec.md. Flags likely semantic
duplicates and capability-boundary quality concerns as a PR comment,
via the `claude` CLI (Claude Code) running the .claude/skills/capability-review
skill, authenticated with a subscription token (CLAUDE_CODE_OAUTH_TOKEN) rather
than a metered ANTHROPIC_API_KEY. Never fails the CI run on its own -- a
missing token, missing CLI, or any tooling error degrades to a
"review didn't run" comment per FR-003.

Usage: ai_advisory_review.py <changed-contract-json-path> <output-comment-path>
"""

import json
import shutil
import subprocess
import sys
from pathlib import Path

DEGRADED_COMMENT = (
    "## AI-Advisory Review (non-blocking)\n\n"
    "This pass did not run (claude CLI unavailable, CLAUDE_CODE_OAUTH_TOKEN not "
    "configured, or the call failed). This does not block merge -- see "
    "specs/004-ai-advisory-review/spec.md FR-003."
)


def load_existing_summaries(index_path: Path) -> list:
    if not index_path.is_file():
        return []
    try:
        index = json.loads(index_path.read_text())
    except Exception:
        return []
    return [
        {"namespace": c.get("namespace"), "id": c.get("id"), "version": c.get("version")}
        for c in index.get("capabilities", [])
    ]


def run_capability_review_skill(new_contract: dict, existing_summaries: list) -> str:
    claude_bin = shutil.which("claude")
    if not claude_bin:
        raise RuntimeError("claude CLI not found on PATH")

    prompt = (
        "/capability-review\n\n"
        f"New contract:\n{json.dumps(new_contract, indent=2)}\n\n"
        f"Existing published capabilities (summary only):\n{json.dumps(existing_summaries, indent=2)}"
    )

    # CLAUDE_CODE_OAUTH_TOKEN (subscription auth) is read from the environment
    # by the CLI itself -- not passed as a CLI argument. See
    # specs/004-ai-advisory-review/spec.md and the registry decision log for
    # why this uses subscription auth instead of a metered API key.
    result = subprocess.run(
        [claude_bin, "-p", prompt],
        capture_output=True,
        text=True,
        timeout=120,
        check=True,
    )
    return result.stdout.strip()


def main() -> int:
    if len(sys.argv) != 3:
        print("Usage: ai_advisory_review.py <changed-contract-json-path> <output-comment-path>", file=sys.stderr)
        return 1

    contract_path = Path(sys.argv[1])
    output_path = Path(sys.argv[2])

    try:
        new_contract = json.loads(contract_path.read_text())
        existing_summaries = load_existing_summaries(Path("index.json"))
        comment = run_capability_review_skill(new_contract, existing_summaries)
        if not comment:
            raise RuntimeError("claude CLI returned empty output")
    except Exception as exc:
        # FR-003: never fail the CI run on CLI/tooling/auth failure.
        comment = DEGRADED_COMMENT + f"\n\n(diagnostic: {exc})"

    output_path.write_text(comment)
    print(comment)
    return 0


if __name__ == "__main__":
    sys.exit(main())
