# Legacy Passthrough: `044-application-bundle-manifest`

This is not a spec document — it is a minimal, machine-readable passthrough
entry required by `specs/014-extraction-compatibility/spec.md` FR-001.

The extracted `traverse-registry` crate's compiled source hardcodes this
exact literal spec-ID string (originally approved in
`traverse-framework/traverse`'s own governance registry) and validates it
at runtime via `approved_spec_registry_contains`. Registering this ID here
preserves that pre-existing behavior across the crate's physical
relocation from `traverse-framework/traverse` to this repo, without any
change to the crate's source.

Full context: `specs/014-extraction-compatibility/spec.md`,
`docs/decision-log.md` entry 33.
