# Catalog (registry#105 / registry#103)

`index.html` is the thin static template referenced in registry#105's design
(decision-log entry 40): plain HTML/CSS/vanilla JS, no build step, no
framework. Styled to match `traverse-framework/website`'s own design tokens
(dark purple/orange theme, Space Grotesk/Inter/JetBrains Mono, card/badge/
code-block components) rather than inventing a separate visual language, with
a light-mode toggle.

It fetches `./catalog.json` at page-load time and renders:

- A **list view** grouped one card per `namespace/id` (not one per version --
  each card shows the current/highest version, a "N versions" badge, a
  coverage badge, and a use-case count), with deprecated capabilities shown
  by default (a checkbox hides them) and free-text search across
  `search_index`.
- A hash-routed (`#/capability/<namespace>/<id>@<version>`, shareable)
  **detail view** per capability version: description, a prominent **use
  cases** section (color-coded happy/unhappy, real input/output JSON),
  **test coverage** (real `cargo llvm-cov` line/function/region percentages
  and test count -- see below), an **interface** section (input/output JSON
  schemas plus execution constraints and permissions), a **version history**
  list linking every version of that same capability id (marking the current
  and any deprecated ones), and a collapsible raw `contract.json`.

Client-side routing was chosen for this interactive experience because the
WASM ABI only allows a single output stream (`fd_write`) per invocation --
`catalog-builder` can only ever produce one JSON document, not N files.
**However**, hash-fragment "pages" (`#/capability/...`) are invisible to
search crawlers and link-unfurlers -- they never see the fragment or execute
this page's JavaScript, so every one looks identical with the same generic
meta tags. Registry#131 addresses this with a *second*, static rendering
path -- see below -- rather than by trying to make crawlers understand hash
routes.

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
      "contract": { "namespace": "validation", "id": "validation.validate-luhn", "version": "1.1.0", "...": "the full contract.json" },
      "test_coverage": { "lines_percent": 98.8, "functions_percent": 100.0, "regions_percent": 99.3, "test_count": 5 }
    }
  ],
  "search_index": {
    "luhn": ["validation/validation.validate-luhn@1.1.0"]
  }
}
```

`test_coverage` is **real, measured data** (`cargo llvm-cov --json --summary-only`
against the crate under `capability-src/` whose source currently backs the
capability), not a fabricated or assumed number -- and it is attached only to
whichever version of each id is *current* (the one whose logic still lives in
`capability-src/`; older/deprecated versions get `null`, since their actual
implementation isn't retained separately in this repo to measure honestly).
See `scripts/ci/gather_catalog_data.py`'s `CURRENT_CRATE_FOR_ID` mapping.
Requires `cargo-llvm-cov` (and the `llvm-tools-preview` rustup component) on
`PATH` -- the `build-catalog` CI job installs it via
`taiki-e/install-action@cargo-llvm-cov`.

Note: `capability-src/wasi-capability-runtime`'s bump-allocator heap was
raised from 1 MiB to 16 MiB to fit this -- the original size assumed one
small request/response, not the whole `capabilities/` tree's contracts held
in memory at once (input parse tree + a full-contract-cloning output tree +
the serialized JSON output, none of it freed). This is a shared-crate change
so it also (harmlessly) applies to every other capability's future rebuild;
zero-initialized static memory costs nothing in the compiled `.wasm` and a
larger declared-but-unused linear memory reservation is cheap at
instantiation.

## SEO: static per-capability pages, sitemap, robots.txt (registry#131)

`scripts/ci/generate_catalog_pages.py` reads the same `catalog.json` the SPA
fetches and emits one **real, statically rendered** HTML file per published
capability version at `catalog/capability/<namespace>/<id>/<version>/index.html`
-- its own `<title>`, `<meta name="description">`, Open Graph tags, and
`<link rel="canonical">`, populated from that capability's own
`summary`/`description`, plus real body content (use cases, schemas, version
history, etc.) mirroring the SPA's detail view. These are genuinely separate,
crawlable documents -- unlike the SPA's hash routes, a search engine or link
preview bot gets distinct, correct metadata per capability without executing
any JavaScript. The script also writes `catalog/sitemap.xml` listing the root
page plus every generated capability page.

```bash
python3 scripts/ci/generate_catalog_pages.py catalog/catalog.json https://registry.traverse-framework.com catalog
```

`catalog/robots.txt` is a plain, hand-written static file (its content
doesn't depend on capability data) allowing full crawl and pointing at the
sitemap.

The SPA's own detail view gained a **Permalink** field linking to this same
static page for whichever version is being viewed, so a user of the
interactive catalog has an obvious, correct URL to copy/share instead of the
hash-fragment one.

**Out of scope, disclosed**: workflows aren't included -- the catalog
pipeline (`gather_catalog_data.py`/`catalog-builder`) doesn't process
`workflows/` at all yet (registry#124's own disclosed gap), so there's
nothing to statically render for them until that lands.
