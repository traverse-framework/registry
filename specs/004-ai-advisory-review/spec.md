# Feature Specification: AI-Advisory PR Review

**Feature Branch**: `004-ai-advisory-review`
**Created**: 2026-07-03
**Status**: Approved
**Input**: Registry ticket #7 — implement the advisory (non-blocking) AI review pass described in `001-registry-foundation` User Story 2 / FR-004.

## Purpose

Defines the concrete tooling and behavior for the AI-advisory pass, resolving the "model/tooling/prompt design" question `001-registry-foundation` deliberately deferred.

## Design Decisions

### Tooling

A CI job that calls the Claude API (Anthropic) with a prompt containing: the new `contract.json`'s full content, and a compact summary (id, namespace, description, tags) of every currently-published capability from the latest `index.json`. This avoids needing a vector database or embeddings pipeline for v1 — the summary list is small enough (in the tens to low hundreds of capabilities) to fit directly in a single prompt for the foreseeable future; revisit if the registry grows beyond a size where this stops being true.

### Prompt Behavior

The model is asked to return a structured (JSON) response with:

- `likely_duplicates`: a list of `{namespace, id, version, reason}` for existing capabilities that appear to overlap in effect with the new one, even if named differently.
- `boundary_concerns`: a list of concerns if the new capability looks like a CRUD wrapper, a utility function, or otherwise fails the "one meaningful business action" test from the org constitution's Principle I.

### Posting Results

CI posts these results as a single PR comment (not a review, not a blocking check) titled "AI-Advisory Review (non-blocking)". The comment explicitly states it does not block merge and that human judgment overrides it.

### Failure Handling

If the API call fails (timeout, rate limit, outage), the job posts a comment noting the advisory pass didn't run and does **not** fail the CI run — this check must never become an accidental blocking gate through infrastructure failure, per `001-registry-foundation`'s Edge Cases section.

## Functional Requirements

- **FR-001**: CI MUST call an LLM with the new contract and a summary of existing capabilities on every PR that adds or modifies a file under `capabilities/`.
- **FR-002**: The job's result MUST be posted as an advisory PR comment, never as a required/blocking status check.
- **FR-003**: A failure of the underlying API call MUST NOT fail the CI run — it must degrade to a "review didn't run" comment.
- **FR-004**: The comment MUST explicitly state that human judgment overrides the AI-advisory result.

## Success Criteria

- **SC-001**: A capability that duplicates an existing one in effect (different name/tags) gets a PR comment identifying the specific likely duplicate.
- **SC-002**: A capability that represents a CRUD wrapper gets a PR comment flagging the boundary concern.
- **SC-003**: Merge remains possible after human approval regardless of what this pass reports.
- **SC-004**: Simulating an API outage during this job does not fail the overall CI run.

## Assumptions

- An API key/credential for the LLM call is provisioned as a repository secret (`ANTHROPIC_API_KEY` or equivalent) — provisioning that secret is an operational step outside this spec's scope, to be done by the repo owner before this job can run for real.
- Cost of the API calls is expected to be small relative to PR volume in this repo's foreseeable future and is not separately budgeted here.
