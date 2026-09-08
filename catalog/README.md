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

`catalog.json` shape (produced by `catalog-builder`, then `contract_url` /
`contract_digest` added per entry by `mirror_artifacts.py` -- see "Browser
verified-retrieval mirror" below) -- each capability entry carries its
**entire source `contract.json`**, not a hand-picked field subset, so the
detail page always has "all the infos" without this pipeline needing to grow
a new field mapping every time the contract schema does:

```json
{
  "capabilities": [
    {
      "reference": "validation/validation.validate-luhn@1.1.0",
      "deprecated": false,
      "contract": { "namespace": "validation", "id": "validation.validate-luhn", "version": "1.1.0", "...": "the full contract.json" },
      "test_coverage": { "lines_percent": 98.8, "functions_percent": 100.0, "regions_percent": 99.3, "test_count": 5 },
      "contract_url": "https://registry.traverse-framework.com/artifacts/validation.validate-luhn-1.1.0/contract.json",
      "contract_digest": "sha256:…",
      "risk": { "effect_class": "pure_read", "determinism_class": "deterministic", "data_flow": { "egress_policy": "denied", "accepted_data_classifications": [], "produced_data_classifications": [] }, "reliability": { "idempotency_required": false, "retryable": true, "compensation_available": false } },
      "is_automatic_eligible": true,
      "risk_source": "declared"
    }
  ],
  "personas": [
    { "reference": "meeting-organizer@1.0.0", "persona": { "...": "the full persona.json" } }
  ],
  "events": [
    {
      "reference": "core/core.action-item.status-transitioned@1.0.0",
      "deprecated": false,
      "product": { "...": "the full events/.../product.json EventProductDescriptor" },
      "observed_lineage": {
        "interactions": [{ "event_id": "...", "role": "publisher", "...": "..." }],
        "drift": [{ "kind": "undeclared_subscriber", "...": "..." }]
      }
    }
  ],
  "search_index": {
    "luhn": ["validation/validation.validate-luhn@1.1.0"]
  }
}
```

Event products (registry#160 / specs/016 FR-014) are listed under `#/events` with
filters for event, capability, domain, owner, lifecycle, and exposure
classification. They are not folded into the capability `search_index`.

`observed_lineage` is fixture-backed for v1 (registry#256 / specs/016 FR-013):
`contracts/governance/observed-lineage-fixture.json` is joined by
`gather_catalog_data.py` and passed through catalog-builder. It is structurally
disjoint from `product` (declared state). Event detail pages show declared
publishers/subscribers vs observed interactions, with a drift badge when
evidence exists.

AsyncAPI documents (specs/016 FR-015) are regenerated into
`catalog/asyncapi/<id>@<version>.json` by the `export_async_api` binary during
the `build-catalog` job; event detail pages link **Download AsyncAPI**.

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

## Browser verified-retrieval mirror (registry#304, registry#383)

`scripts/ci/mirror_artifacts.py` (the `build-catalog` job's "Mirror artifact
WASM + verified contract files" step) re-hosts, under the same CORS-open
Pages origin as `catalog.json`, everything a browser at an arbitrary origin
needs to run `traverse-embedder-web`'s `registryCache` path (spec
`080-embedded-registry-cache`) against the public registry -- fetch **and
digest-verify** a capability's WASM *and* its contract, then hand the
verified bytes to `BundleEmbedder`.

For every published capability version, at a path that is a fixed prefix
swap of the authoritative `contract.artifact.url`
(`.../releases/download/artifacts/<ns>.<id>-<ver>/…` →
`https://registry.traverse-framework.com/artifacts/<ns>.<id>-<ver>/…`):

| File | Contents |
| --- | --- |
| `artifacts/<ns>.<id>-<ver>/<asset>.wasm` | the compiled artifact, bytes re-verified against `contract.artifact.digest` at build time (registry#304) |
| `artifacts/<ns>.<id>-<ver>/contract.json` | the immutable `capabilities/<ns>/<id>/<ver>/contract.json`, copied **verbatim** (registry#383) |
| `artifacts/<ns>.<id>-<ver>/contract.json.sha256` | `sha256:<hex>` of those exact contract bytes |

The same `contract` mirror URL and digest are also added to each
`catalog.json` capability entry as `contract_url` / `contract_digest` (see
the shape example above) so a consumer building a
`SyncedPublicRegistryState` snapshot has them inline. `contract_digest`
uses the identical `"sha256:" + sha256(raw_bytes)` recipe
`scripts/ci/build_index.py` uses for `index.json`'s `contract_digest` (spec
`009-contract-metadata-in-index`), computed over the identical bytes, so the
two public surfaces agree for any given version.

`contract.json`'s own `artifact.digest`/`url` remain the sole authoritative
record per spec `007-artifact-hosting`; this mirror is a convenience
read-path, never referenced by a contract, and is regenerated fresh on
every catalog build (deprecated versions included -- a yanked version must
stay fetchable and verifiable too).

## Capability risk classification (spec 024, registry#384)

Every capability entry in `catalog.json` (and every capability entry in the
GitHub-Release `index.json`) carries three additive fields adopting
`traverse-framework/traverse` Spec 109 FR-005/FR-006:

- `risk` -- the `traverse-contracts::RiskMetadata` object: `effect_class`
  (`pure_read` | `state_write` | `external_effect` | `irreversible_effect`),
  `determinism_class` (`deterministic` | `externally_variable` |
  `model_derived`), `data_flow` (`egress_policy` plus field-level
  accepted/produced data classifications), and `reliability`
  (`idempotency_required` / `retryable` / `compensation_available`).
- `is_automatic_eligible` -- a boolean: `traverse_contracts::is_automatic_eligible`
  applied to `risk` (`pure_read` ∧ `deterministic` ∧ egress `denied` ∧
  ¬`idempotency_required`). Filter to `is_automatic_eligible === true` for the
  subset safe to run unattended and unauthenticated -- no `null`-handling,
  legacy versions are already `false`.
- `risk_source` -- `"declared"` when the contract declares its own `risk`
  block, `"conservative_default"` when it does not (the ~115 versions
  published before spec 024; resolved via
  `traverse_contracts::default_risk_metadata()`, exactly as the runtime
  treats them).

The verdict is **always** computed by the `traverse-registry`
`resolve_capability_risk` binary through `traverse-contracts` --
`gather_catalog_data.py` and `build_index.py` invoke it, `catalog-builder`
passes the result through verbatim. Neither the Python pipeline nor the
`no_std` wasm ever re-derives the rule (spec 024 FR-004). New/changed
contracts must declare a well-formed `risk` block or `capability-validation`
fails (`contract.missing_risk_metadata` / `contract.invalid_risk_metadata`);
`traverse-cli capability publish` does not emit it yet, so add it by hand
until it does.

## Analytics (registry#133, decision-log entry 45)

`catalog/analytics.js` loads [Plausible](https://plausible.io) (hosted,
privacy-first -- no cookies, no PII, no consent banner needed) and exposes
two small helpers, `trackPageview(url)` and `trackSearch(query, resultCount)`.
Both `catalog/index.html` and every page
`scripts/ci/generate_catalog_pages.py` generates load this same script, so
both surfaces report consistently under one account instead of drifting
apart.

- **Page views**: the generated static pages get Plausible's automatic
  pageview tracking for free (each is a real, separate document). The SPA
  additionally fires a manual, URL-overridden pageview on hash navigation
  (attributed to the *same* real permalink URL its static-page counterpart
  uses, via Plausible's `u` override) -- so a capability viewed through
  either surface counts under one URL, not two.
- **Search**: a debounced (600ms after the user stops typing) `Search`
  custom event with `query` and `results` count as properties. Zero-result
  searches aren't specially flagged beyond `results: 0` -- Plausible's own
  dashboard can filter/segment by that property directly.

**This repo does not, and cannot, create the actual Plausible account or
register `registry.traverse-framework.com` with it** -- that's a one-time
setup step for the repo owner to do directly (creating third-party accounts
isn't something this codebase does on anyone's behalf). Until that's done,
these events are sent to an unregistered domain and Plausible silently drops
them; nothing else about the site depends on it or breaks in the meantime.
