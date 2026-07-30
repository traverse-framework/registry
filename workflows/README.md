# Workflows Directory

Published, first-class workflow records (spec 001 FR-013, decision-log entry 44,
closing #99). Governed the same way `capabilities/` is: immutable per version,
validated in `scripts/ci/capability_validation.py`, included in the public index
built by `scripts/ci/build_index.py`.

## Layout

```text
workflows/<namespace>/<id>/<version>/workflow.json
workflows/<namespace>/<id>/<version>/example-request.json   # optional
```

- `<namespace>`/`<id>`/`<version>` follow the exact same rules as `capabilities/`
  (see `capabilities/README.md`) — path segments must match the record's own
  `namespace`/`id`/`version` fields, and a version directory is never edited
  after merge. To retire a workflow version, add a `deprecated.json` sibling
  (spec 005's yank mechanism), never edit `workflow.json` itself.
- `workflow.json` — the workflow definition itself: `inputs`/`outputs` schemas,
  `nodes` (each pinning a `capability_id`/`capability_version` it composes —
  this membership list *is* the canonical "content group" answer per FR-013,
  no separate content-group record type), `edges`, `start_node`,
  `terminal_nodes`.
- `example-request.json` (registry#125) — a standalone, runnable example: a
  `request` object matching the workflow's `inputs.schema` and the
  `expected_response` you should get back from running the full pipeline
  end to end. Lets a consumer `curl`/pipe a real request without hand-composing
  one from each underlying capability's own `use_cases`. Every one published
  here was verified against the real compiled WASM binaries (chained through
  each node in order via `wasmtime run`), not hand-traced.

`workflows/examples/` is **not** real published content — it holds
pre-FR-013 demo/fixture material (`workflows/examples/expedition/`) that
predates this layout and doesn't follow it. `capability_validation.py` and
`build_index.py` both explicitly exclude it, the same way neither script
walks `examples/applications/`.

## Current content (as of 2026-07-29)

Two kit workflows are published, composing capabilities from the reference apps
(see `capabilities/README.md`):

| namespace | id | current version | composes |
|---|---|---|---|
| `traverse-starter` | `traverse-starter.process-note` | 1.0.0 | `validate` -> `process` -> `summarize` |
| `doc-approval` | `doc-approval.review-document` | 1.0.0 | `analyze` -> `recommend` |

`meeting-notes.process` has no natural multi-step pipeline, so it isn't
wrapped in a workflow — per FR-013's explicit single-capability-entrypoint
carve-out, it instead carries a boolean `entrypoint: true` marker directly on
its own contract (`capabilities/meeting-notes/meeting-notes.process/1.3.0/`),
plus its own standalone `example-request.json` for the same "runnable
example" convenience the two workflows get.
