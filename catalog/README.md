# Catalog (registry#105 / registry#103)

`index.html` is the thin static template referenced in registry#105's design
(decision-log entry 40): plain HTML/CSS/vanilla JS, no build step, no
framework. It fetches `./catalog.json` at page-load time and renders a
searchable list of every published capability.

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

`catalog.json` shape (produced by `catalog-builder`):

```json
{
  "capabilities": [
    {
      "namespace": "validation",
      "id": "validation.validate-luhn",
      "version": "1.0.0",
      "summary": "...",
      "deprecated": false,
      "reference": "validation/validation.validate-luhn@1.0.0"
    }
  ],
  "search_index": {
    "luhn": ["validation/validation.validate-luhn@1.0.0"]
  }
}
```
