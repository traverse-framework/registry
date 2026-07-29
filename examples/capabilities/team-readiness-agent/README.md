# team-readiness-agent (example fixture)

Vendored verbatim from `traverse-framework/traverse`'s
`examples/capabilities/team-readiness-agent/` (digest verified identical:
`sha256:e975c0eac1491c7c9e7dc004fac8aecb209f5fe238ae1832364e0de10fd4b48a`),
to fix the `examples/applications/expedition-readiness` component manifests'
`wasm_binary_path` referencing a nonexistent local artifact (registry#110).

This is a **fixed-output fixture**, not real per-input logic -- `src/agent.rs`
always writes the same hardcoded `OUTPUT` regardless of input, same pattern
disclosed for the reference-app capabilities before their real-logic rewrite
(see `docs/decision-log.md` entries 34-35). It backs all five
`expedition-readiness` example components (`validate-team-readiness`,
`capture-expedition-objective`, `interpret-expedition-intent`,
`assess-conditions-summary`, `assemble-expedition-plan`), matching the
upstream example's own precedent of sharing one placeholder binary across all
five. Rebuild with `./build-fixture.sh` (requires `rustup` with a
`wasm32-unknown-unknown` target).

This is example/demonstration content for `examples/applications/`, not a
published registry capability -- it is not subject to `capabilities/README.md`'s
"never commit artifacts directly" rule, which governs the immutable
`capabilities/<namespace>/<id>/<version>/contract.json` publish pipeline.
