#!/usr/bin/env python3
"""AI-advisory PR review pass (non-blocking).

Implements specs/004-ai-advisory-review/spec.md. Flags likely semantic
duplicates and capability-boundary quality concerns as a PR comment.
Never fails the CI run on its own -- an API error degrades to a
"review didn't run" comment per FR-003.

Uses a metered ANTHROPIC_API_KEY (Claude Console workspace key), not a
subscription OAuth token -- CI usage is billed and isolated separately
from any personal Claude Code/Pro-Max usage. See docs/decision-log.md
item 18 for why the subscription-auth alternative was reverted.

Usage: ai_advisory_review.py <changed-contract-json-path> <output-comment-path>
"""

import json
import os
import sys
import urllib.request
from pathlib import Path

ANTHROPIC_API_URL = "https://api.anthropic.com/v1/messages"
MODEL = "claude-sonnet-4-5"

DEGRADED_COMMENT = (
    "## AI-Advisory Review (non-blocking)\n\n"
    "This pass did not run (API call failed or no credential configured). "
    "This does not block merge -- see specs/004-ai-advisory-review/spec.md FR-003."
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


def call_claude(new_contract: dict, existing_summaries: list) -> dict:
    api_key = os.environ.get("ANTHROPIC_API_KEY")
    if not api_key:
        raise RuntimeError("ANTHROPIC_API_KEY not set")

    prompt = (
        "You are reviewing a new capability contract being published to a governed "
        "capability registry. A valid capability represents ONE meaningful business "
        "action, not a CRUD wrapper or utility function.\n\n"
        f"New contract:\n{json.dumps(new_contract, indent=2)}\n\n"
        f"Existing published capabilities (summary only):\n{json.dumps(existing_summaries, indent=2)}\n\n"
        "Respond with JSON only, matching this shape:\n"
        '{"likely_duplicates": [{"namespace": "", "id": "", "version": "", "reason": ""}], '
        '"boundary_concerns": [""]}'
    )

    body = json.dumps(
        {
            "model": MODEL,
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": prompt}],
        }
    ).encode("utf-8")

    request = urllib.request.Request(
        ANTHROPIC_API_URL,
        data=body,
        headers={
            "content-type": "application/json",
            "x-api-key": api_key,
            "anthropic-version": "2023-06-01",
        },
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        payload = json.loads(response.read())

    text = payload["content"][0]["text"]
    return json.loads(text)


def format_comment(result: dict) -> str:
    lines = ["## AI-Advisory Review (non-blocking)", ""]
    lines.append(
        "This is advisory only. It does not block merge -- human review is always the final gate "
        "(see specs/004-ai-advisory-review/spec.md FR-002, FR-004)."
    )
    lines.append("")

    duplicates = result.get("likely_duplicates") or []
    if duplicates:
        lines.append("### Likely duplicates")
        for dup in duplicates:
            lines.append(
                f"- `{dup.get('namespace')}/{dup.get('id')}@{dup.get('version')}` -- {dup.get('reason')}"
            )
    else:
        lines.append("### Likely duplicates\nNone flagged.")

    lines.append("")
    concerns = result.get("boundary_concerns") or []
    if concerns:
        lines.append("### Capability-boundary concerns")
        for concern in concerns:
            lines.append(f"- {concern}")
    else:
        lines.append("### Capability-boundary concerns\nNone flagged.")

    return "\n".join(lines) + "\n"


def main() -> int:
    if len(sys.argv) != 3:
        print("Usage: ai_advisory_review.py <changed-contract-json-path> <output-comment-path>", file=sys.stderr)
        return 1

    contract_path = Path(sys.argv[1])
    output_path = Path(sys.argv[2])

    try:
        new_contract = json.loads(contract_path.read_text())
        existing_summaries = load_existing_summaries(Path("index.json"))
        result = call_claude(new_contract, existing_summaries)
        comment = format_comment(result)
    except Exception as exc:
        # FR-003: never fail the CI run on API/tooling failure.
        comment = DEGRADED_COMMENT + f"\n\n(diagnostic: {exc})"

    output_path.write_text(comment)
    print(comment)
    return 0


if __name__ == "__main__":
    sys.exit(main())
