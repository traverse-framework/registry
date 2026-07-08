---
name: "capability-review"
description: "Run the AI-advisory review of a capability publish PR interactively in chat when the user says CAPABILITY REVIEW, asks to review a publish PR, or asks for duplicate/boundary-quality analysis of a new capability contract. Advisory only -- human judgment always overrides it."
---

# Capability Review (in-chat advisory)

Use this skill to perform the AI-advisory review defined by
`specs/004-ai-advisory-review/spec.md` **in the reviewer's chat session**
instead of CI. Per `docs/decision-log.md` entries 19 and 25, the repo owner
does not provision `ANTHROPIC_API_KEY` for CI; the CI job
(`ai-advisory-review` in `.github/workflows/ci.yml`) stays wired but
degrades to a "review didn't run" notice by design. This skill is where the
advisory analysis actually happens.

**This review is advisory only. It never blocks merge, and human judgment
always overrides it** (spec 004 FR-002, FR-004).

## Input

One of:

- a PR number against `traverse-framework/registry` (the normal case)
- a path to (or diff adding) a `capabilities/<namespace>/<id>/<version>/contract.json`

## Workflow

1. **Extract the new contract(s).** For a PR:
   `gh pr diff <N> --repo traverse-framework/registry --name-only` and take
   the added `capabilities/**/contract.json` files; read each one's full
   JSON (from the PR head, e.g. `gh api` the contents at the head sha, or
   the local branch if checked out). If the PR adds no contract file,
   report that there is nothing for this skill to review and stop.
2. **Load the published-capability summaries.** Download the latest index
   release (`gh release download <latest index-vN tag> --repo
   traverse-framework/registry --pattern index.json`) and reduce it to the
   same compact summary shape `scripts/ci/ai_advisory_review.py` uses:
   `{namespace, id, version}` per published capability. If a summary looks
   like a plausible overlap by name alone, read that capability's full
   `contract.json` from `capabilities/` before deciding.
3. **Analyze.** Apply the same rubric as the prompt in
   `scripts/ci/ai_advisory_review.py` (that script is the canonical wording
   -- do not invent a divergent second rubric):
   - A valid capability represents **one meaningful business action**, not
     a CRUD wrapper or utility function.
   - **Likely duplicates**: existing published capabilities whose effect
     semantically overlaps the new contract even under a different
     name/tags -- identify each as `namespace/id@version` with a reason.
   - **Boundary concerns**: signs the contract is a CRUD wrapper, a
     utility/helper disguised as a capability, or otherwise a poor
     capability boundary -- each with a reason.
4. **Report** in the same shape as the script's `format_comment` output:

   ```markdown
   ## AI-Advisory Review (non-blocking)

   This is advisory only. It does not block merge -- human review is always
   the final gate (see specs/004-ai-advisory-review/spec.md FR-002, FR-004).

   ### Likely duplicates
   - `namespace/id@version` -- reason        (or "None flagged.")

   ### Capability-boundary concerns
   - concern with reason                     (or "None flagged.")
   ```

5. **Post to the PR only if asked.** Default is reporting in chat; if the
   reviewer wants it on the record, post the same markdown via
   `gh pr comment <N> --body-file ...`.

## Guardrails

- Never state or imply the review gates merge -- it informs the human
  reviewer, who may disagree and merge anyway.
- Do not modify `.github/workflows/ci.yml` or
  `scripts/ci/ai_advisory_review.py` as part of running a review -- the
  degraded CI notice on publish PRs is expected (decision 19), not a bug to
  fix.
- Review only what the PR adds; deterministic validation (schema, semver,
  digest, immutability, dependency resolvability) belongs to
  `scripts/ci/capability_validation.py`, not this skill -- don't duplicate
  its verdicts, though you may note if something looks off.
