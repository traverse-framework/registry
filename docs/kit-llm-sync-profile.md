# The "kit-llm" sync profile

Registry#126, child of #99. Decision-log entry 44 resolved this as a
**documented convention over the existing `registry sync` + index
mechanism** — deliberately not a new registry-side artifact, release
process, or bundle type. This page is that documentation.

## What "kit" means

The **kit** is the curated set of reference-app content meant for an LLM/MCP
consumer that wants a small, known-good, real-logic starting point — not
the entire public index. It is exactly these namespaces:

- `traverse-starter`
- `doc-approval`
- `meeting-notes`
- `core` (reserved for future general-purpose capabilities with no natural
  reference app — see `capabilities/README.md`'s namespace policy note; none
  are published under it yet)

**Everything else in the index is explicitly not kit content**, most
notably the five `validation`/`formatting` utility-tier capabilities
(`capabilities/README.md`'s "Utility-tier capabilities" section) — real,
published, and perfectly usable, just not part of this curated set. A
kit-LLM consumer that wants those too should read the full index, not the
kit-filtered subset this page describes.

## The two entrypoint shapes

A kit-LLM consumer has exactly two kinds of top-level thing to run:

1. **A workflow** (`workflows[]` in the index) — a multi-step pipeline. Two
   exist today:
   - `traverse-starter.process-note` (`validate` -> `process` -> `summarize`)
   - `doc-approval.review-document` (`analyze` -> `recommend`)

   Each workflow's own `workflow.json` (fetch via the index entry's
   `workflow_url`) lists its composing capabilities in `nodes[]` — that
   membership list *is* the answer to "what does this entrypoint actually
   run" (spec 001 FR-013); there is no separate content-group record to
   cross-reference.

2. **A single-capability entrypoint** (`capabilities[]`, no matching
   workflow) — a capability with no natural multi-step pipeline, marked
   with a boolean `"entrypoint": true` field **directly on its contract**,
   not in the lightweight index entry itself. Today: `meeting-notes.process`.

   **Important**: the index's `capabilities[]` entry does not inline
   `entrypoint` (see the shape below) — a consumer must fetch the full
   `contract.json` via the entry's `contract_url` to check for it. Don't
   assume an index entry alone tells you whether a capability is a kit
   entrypoint.

Every kit entrypoint (both workflows and the single-capability one) also
ships a standalone, runnable `example-request.json` sibling (#125) — a real
`request`/`expected_response` pair, so a consumer can try one immediately
without composing a request from a contract's `use_cases` by hand.

## The sync + filter steps

No new CLI flag or registry-side mechanism — this is exactly `registry
sync` plus a client-side filter over the two arrays it already produces:

1. Run `traverse-cli registry sync` (or point it at this repo via
   `--source-repo`/`--registry-repo-remote`, per decision-log entry 39) to
   fetch the latest `index-vN` release into local durable workspace state.
2. Load the synced `index.json`. Its relevant shape (abbreviated):

   ```json
   {
     "capabilities": [
       {
         "namespace": "doc-approval",
         "id": "doc-approval.analyze",
         "version": "1.3.0",
         "digest": "sha256:...",
         "artifact_url": "https://...analyze-agent.wasm",
         "contract_digest": "sha256:...",
         "contract_url": "https://...contract.json",
         "deprecated": false
       }
     ],
     "workflows": [
       {
         "namespace": "doc-approval",
         "id": "doc-approval.review-document",
         "version": "1.0.0",
         "workflow_digest": "sha256:...",
         "workflow_url": "https://...workflow.json",
         "deprecated": false
       }
     ]
   }
   ```

3. Filter both arrays to `namespace` in `{core, traverse-starter,
   doc-approval, meeting-notes}` — e.g. with `jq`:

   ```bash
   kit_namespaces='["core","traverse-starter","doc-approval","meeting-notes"]'
   jq --argjson ns "$kit_namespaces" '{
     capabilities: [.capabilities[] | select(.namespace as $n | $ns | index($n))],
     workflows:    [.workflows[]    | select(.namespace as $n | $ns | index($n))]
   }' index.json
   ```

4. Within the filtered `capabilities[]`, exclude deprecated versions unless
   you specifically want historical/pinned access (spec 001 FR-014: every
   fixture/stub version is deprecated the moment its real successor
   publishes, so `^1`-style resolution already lands on real logic without
   any extra filtering — this step is only needed if you're working from
   the raw array directly instead of through a resolver).
5. For each remaining workflow, fetch `workflow_url` to get its `nodes[]`
   (which capabilities it composes) and its sibling `example-request.json`.
   For each remaining capability you want to check as a possible
   single-capability entrypoint, fetch `contract_url` and look for
   `entrypoint: true`.

That's the whole profile: sync, filter by namespace, resolve non-deprecated,
fetch the two entrypoint shapes' details on demand. No bundle, no second
publish pipeline, no new index schema beyond the `workflows[]` array #124
already added.
