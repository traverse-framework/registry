# Feature Specification: Registry-Owned Public Execution Proxy

**Feature Branch**: `claude/registry-owned-execution-proxy`
**Created**: 2026-08-26
**Status**: Draft
**Input**: Owner-co-brainstormed session, motivated by `traverse-framework/traverse#1158` ("Browser: execute one verified published WASM capability and show trace evidence") being blocked on how a public, anonymous browser client can reach `traverse-cli serve`'s verified-entrypoint execution endpoint (traverse spec `115-browser-verified-entrypoint-execution`) given traverse spec `033-http-json-api` FR-035 deliberately gates system-workspace access behind a signature-verified admin token.

## Purpose

`traverse-cli serve`'s verified-entrypoint execution (traverse specs
`115`/`118`/`120`, all Approved and implemented) always runs under
`SYSTEM_WORKSPACE_ID`, which traverse spec `033` FR-035 correctly and
deliberately requires a real, signature-verified admin credential to reach —
this is governed, intentional behavior, not a gap to route around. A static
site with no backend of its own (`discover.html`) cannot hold that
credential safely: anything embedded in public client-side JS is
extractable and reusable by anyone who views the page source.

This spec defines a thin, registry-owned proxy that holds the real admin
credential privately, validates and rate-limits incoming requests, and
forwards only what it has validated to a `traverse-cli serve` instance it
also operates. Neither `traverse-cli serve`'s trust model nor any
traverse-side spec changes — the proxy is simply a new, ordinary
admin-scoped client of an already-governed, already-approved surface.

Ownership sits with this repository (the registry) rather than the
`website` or `traverse` repos because this repository already operates
real, deployed public infrastructure (`registry.traverse-framework.com`)
and a release pipeline — this proxy extends that existing operational
surface instead of requiring a new team to stand up infrastructure from
scratch.

## Design Decisions

### Proxy holds the credential; the browser never does

The browser calls a public, unauthenticated proxy endpoint. The proxy
alone holds a real, signature-verified admin JWT (provisioned and rotated
as an ordinary operational secret, out of this spec's scope) and uses it
to call `traverse-cli serve`'s verified-entrypoint endpoint on the caller's
behalf. The proxy never returns that credential, or any part of it, in any
response.

### Request validation happens before forwarding, not just at `serve`

`serve` already enforces `LookupScope::PublicOnly` resolution and its own
input validation (spec 115 FR-002/FR-003). The proxy adds a second,
independent layer in front of that: it checks the requested
`entrypoint_kind`/`id`/`version` against the same public capability list
`index.json` already publishes before ever forwarding a request. This is
deliberate defense in depth, not redundant — it means a malformed or
out-of-scope request never reaches `serve` (and never spends real
execution resources) in the first place.

### Rate limiting is part of this spec, not a follow-up

A public, credential-free endpoint that triggers real WASM execution on
paid infrastructure is a resource-exhaustion target the moment it exists.
Per-IP/per-session rate limiting is a Functional Requirement here, not an
operational afterthought left for whoever deploys it — a spec that
described only "the proxy holds a token and forwards requests" would not
be safe to build against.

### Scope: single-capability execution only

This spec covers exactly what `traverse#1158` needs: one verified
capability, one request, one response. `traverse#1159`'s multi-capability,
reviewed workflow-proposal journey is a distinct concern (it involves
proposal authorization, not just execution) and is explicitly out of scope
here, though it may reuse this proxy's infrastructure later.

## Functional Requirements

- **FR-001**: The proxy MUST expose one public HTTP endpoint accepting an
  unauthenticated request naming an `entrypoint_kind`, exact `id`, exact
  `version`, and an inline execution request body.
- **FR-002**: Before forwarding, the proxy MUST validate that the named
  `entrypoint_kind`/`id`/`version` exactly matches a non-deprecated entry
  in this registry's own published public index (`index.json`). A
  non-matching request MUST be rejected with a stable, actionable error
  and MUST NOT be forwarded to `serve`.
- **FR-003**: The proxy MUST enforce a per-IP (or per-session, if a session
  concept is introduced) rate limit before forwarding any request to
  `serve`. Requests exceeding the limit MUST be rejected with a stable
  `429`-equivalent error, not silently dropped or queued indefinitely.
- **FR-004**: The proxy MUST reject oversized or malformed request bodies
  before forwarding, using limits at or below whatever `serve` itself
  enforces (traverse spec `033`'s existing request-size limits).
- **FR-005**: The proxy MUST hold its `serve` credential (a real,
  signature-verified admin JWT per traverse spec `033` FR-035) privately.
  It MUST NOT expose that credential, or any derivative of it, in any
  response, log line, or error message reachable by the caller.
- **FR-006**: The proxy MUST forward a validated request to `serve`'s
  verified-entrypoint endpoint (traverse spec `115`) unmodified in
  substance, and MUST relay `serve`'s success or redacted-trace-bearing
  error response back to the caller without adding or removing execution
  semantics.
- **FR-007**: This spec introduces no change to any traverse-side spec,
  crate, or endpoint. The proxy authenticates as an ordinary admin-scoped
  `serve` client exactly as traverse specs `033`/`115`/`118`/`120` already
  allow.

## Success Criteria

- **SC-001**: A browser client with no credential of any kind can call the
  proxy and receive a genuine execution result for a real, published,
  non-deprecated capability.
- **SC-002**: A request naming a capability not present (or deprecated) in
  `index.json` is rejected by the proxy before any call reaches `serve`.
- **SC-003**: A caller exceeding the configured rate limit is rejected with
  a stable error, and no admin credential material ever appears in any
  proxy response.
- **SC-004**: `traverse-cli serve`'s own code, specs, and trust model are
  unchanged by this work.

## Assumptions

- Which hosting platform or implementation language the proxy runs on
  (e.g. an edge function vs. a small long-running service) is an
  operational decision for whoever implements this, not governed here —
  matching traverse spec `120`'s own precedent of leaving "how often to
  re-run `materialize`" as an explicit operational non-goal.
- Provisioning, storage, and rotation of the proxy's own admin JWT signing
  material is ordinary secret management, out of this spec's scope.
- Keeping the `serve` instance's registry-state/artifact-state current
  (re-running traverse's `registry sync` / `registry materialize`, per
  traverse specs `117`/`120`) is an operational responsibility of whoever
  operates the paired `serve` instance, not defined here.
- This spec assumes a `traverse-cli serve` instance (with `--registry-state`
  and `--artifact-state`, per traverse specs `118`/`120`) is already
  running and reachable by the proxy; standing that up is a prerequisite,
  not something this spec itself performs.

## Approval

Drafted by an agent from a live, owner-co-brainstormed session (options and
tradeoffs presented one decision at a time; the owner chose the
registry-owned-proxy direction and confirmed rate limiting belongs in this
spec's Definition of Done). Per this repository's no-self-approval-of-specs
rule, this spec stays `Draft` pending the repo owner's own explicit,
standalone sign-off, separate from having co-brainstormed its content —
matching the precedent already recorded in `specs/019-public-metadata-sync-extension`'s own Approval section.
