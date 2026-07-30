# Catalog (registry#105 / registry#103)

`index.html` is the thin static template referenced in registry#105's design
(decision-log entry 40): plain HTML/CSS/vanilla JS, no build step, no
framework. It fetches `./catalog.json` at page-load time and renders a
searchable list of every published capability (deprecated versions included
by default -- a checkbox lets a viewer hide them), plus a hash-routed
(`#/capability/<namespace>/<id>@<version>`) detail view per capability with
its full contract: description, input/output schemas, `use_cases`, owner,
artifact digest/URL, and a collapsible raw `contract.json`. Client-side
routing was chosen over generating one static HTML file per capability
because the WASM ABI only allows a single output stream (`fd_write`) per
invocation -- `catalog-builder` can only ever produce one JSON document, not
N files.

`catalog.json` is **generated, not checked in** -- produced by piping
`scripts/ci/gather_catalog_data.py`'s output through the `catalog-builder`
WASM binary (`capability-src/catalog-builder/`, not a published
`capabilities/` capability, see that crate's `Cargo.toml`):

```bash
python3 scripts/ci/gather_catalog_data.py /tmp/gathered.json
wasmtime run capability-src/catalog-builder/target/wasm32-unknown-unknown/release/catalog-builder.wasm \
  < /tmp/gathered.json > catalog/catalog.json
```

Wiring this into GitHub Actions -> GitHub Pages on every merge to `main` is
registry#106's job, not this one -- see that issue for the deployment side
(`actions/upload-pages-artifact` + `actions/deploy-pages`, and the
repo-owner-only GitHub Pages settings toggle).

`catalog.json` shape (produced by `catalog-builder`) -- each capability entry
carries its **entire source `contract.json`**, not a hand-picked field
subset, so the detail page always has "all the infos" without this pipeline
needing to grow a new field mapping every time the contract schema does:

```json
{
  "capabilities": [
    {
      "reference": "validation/validation.validate-luhn@1.1.0",
      "deprecated": false,
      "contract": { "namespace": "validation", "id": "validation.validate-luhn", "version": "1.1.0", "...": "the full contract.json" }
    }
  ],
  "search_index": {
    "luhn": ["validation/validation.validate-luhn@1.1.0"]
  }
}
```

Note: `capability-src/wasi-capability-runtime`'s bump-allocator heap was
raised from 1 MiB to 16 MiB to fit this -- the original size assumed one
small request/response, not the whole `capabilities/` tree's contracts held
in memory at once (input parse tree + a full-contract-cloning output tree +
the serialized JSON output, none of it freed). This is a shared-crate change
so it also (harmlessly) applies to every other capability's future rebuild;
zero-initialized static memory costs nothing in the compiled `.wasm` and a
larger declared-but-unused linear memory reservation is cheap at
instantiation.
