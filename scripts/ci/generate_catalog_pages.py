#!/usr/bin/env python3
"""Generate real, static per-capability HTML pages + sitemap.xml for the
discovery catalog (registry#131).

Why this exists: catalog/index.html is a client-side, hash-routed SPA
(`#/capability/<ref>`). Search crawlers and link-unfurlers (Slack, Twitter,
etc. -- anything reading Open Graph tags for a link preview) never execute
that JavaScript and never see the URL fragment, so every hash "page" looks
identical to the one generic index.html with the same generic meta tags.
A sitemap listing fragment URLs, or per-page <meta name="description">/
og:* tags baked only into the SPA's DOM, would not actually work for any of
that -- this script instead emits one genuinely separate, statically
rendered HTML document per published capability version, each with its own
title/description/OG/canonical tags and real body content (not a stub),
at a real path GitHub Pages serves directly:

    catalog/capability/<namespace>/<id>/<version>/index.html

The SPA at catalog/index.html is untouched and remains the primary
interactive experience (search, live theme toggle, etc.) -- these generated
pages are the crawlable/shareable counterpart, and each links back to both
the main catalog and its own interactive equivalent
(catalog/index.html#/capability/<reference>).

Reuses catalog/style.css (the same stylesheet catalog/index.html links to)
so the two never visually drift apart. Workflows are out of scope here --
the catalog pipeline (gather_catalog_data.py / catalog-builder) doesn't
process workflows/ at all yet (see registry#124's own disclosed gap), so
there is nothing to statically render for them until that lands.

Usage: generate_catalog_pages.py <catalog.json> <base_url> <output_dir>
"""

import html
import json
import sys
from pathlib import Path


def esc(value) -> str:
    return html.escape(str(value), quote=True)


def field_row(label: str, value) -> str:
    if not value:
        return ""
    return f'<div class="field-row"><span class="field-label">{esc(label)}</span><span class="field-value">{esc(value)}</span></div>'


def json_block(value) -> str:
    return f'<div class="code-block"><pre>{esc(json.dumps(value, indent=2))}</pre></div>'


def use_case_block(use_case: dict) -> str:
    happy = use_case.get("happy", True) is not False
    css_class = "use-case" if happy else "use-case unhappy"
    return (
        f'<div class="{css_class}">'
        f'<p class="use-case-scenario">{esc(use_case.get("scenario", ""))}</p>'
        f'<div class="use-case-io">'
        f'<div><div class="io-label">Input</div>{json_block(use_case.get("input_example"))}</div>'
        f'<div><div class="io-label">Output</div>{json_block(use_case.get("output_example"))}</div>'
        f"</div></div>"
    )


def coverage_block(coverage) -> str:
    if not coverage:
        return (
            '<p class="coverage-note">Not measured for this historical version -- '
            "only the current version's logic is retained in this repo's source tree.</p>"
        )

    def stat(value, label):
        return f'<div class="stat-card"><div class="stat-value">{esc(value)}</div><div class="stat-label">{esc(label)}</div></div>'

    return '<div class="coverage-grid">' + "".join(
        [
            stat(f"{coverage['lines_percent']}%", "Lines"),
            stat(f"{coverage['functions_percent']}%", "Functions"),
            stat(f"{coverage['regions_percent']}%", "Regions"),
            stat(str(coverage["test_count"]), "Tests"),
        ]
    ) + "</div>"


def semver_key(version: str):
    return tuple(int(part) for part in version.split("."))


def capability_page_path(contract: dict) -> str:
    return f"capability/{contract['namespace']}/{contract['id']}/{contract['version']}/"


def version_history_block(base_url: str, group_versions: list, current_reference: str) -> str:
    sorted_versions = sorted(group_versions, key=lambda v: semver_key(v["contract"]["version"]), reverse=True)
    latest_reference = sorted_versions[0]["reference"]
    rows = []
    for entry in sorted_versions:
        contract = entry["contract"]
        badges = f'<span class="t-mono">v{esc(contract["version"])}</span>'
        if entry["reference"] == latest_reference:
            badges += ' <span class="badge badge-accent">latest</span>'
        if entry["deprecated"]:
            badges += ' <span class="badge badge-danger">deprecated</span>'
        href = f"{base_url}/{capability_page_path(contract)}"
        is_current = entry["reference"] == current_reference
        row_class = "version-row current" if is_current else "version-row"
        label = "viewing" if is_current else "view →"
        rows.append(
            f'<a class="{row_class}" href="{esc(href)}"><div class="version-left">{badges}</div>'
            f'<span class="t-muted" style="font-size:0.8rem">{label}</span></a>'
        )
    return "\n".join(rows)


PAGE_TEMPLATE = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<meta name="description" content="{description}">
<meta property="og:type" content="website">
<meta property="og:title" content="{title}">
<meta property="og:description" content="{description}">
<meta property="og:url" content="{canonical_url}">
<link rel="canonical" href="{canonical_url}">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Space+Grotesk:wght@500;600;700&family=Inter:wght@300;400;500;600&family=JetBrains+Mono:wght@400;500&display=swap" rel="stylesheet">
<link rel="stylesheet" href="/style.css">
</head>
<body>
<nav class="nav">
  <div class="nav-inner">
    <a href="/" class="nav-logo"><span>Traverse<span class="nav-logo-dot">.</span></span><span class="nav-label">REGISTRY CATALOG</span></a>
  </div>
</nav>
<div class="container">
<a class="back-link" href="/">← Back to catalog</a>
<div class="detail-header-badges">{header_badges}</div>
<h1 class="t-h1 detail-title">{id}</h1>
{summary_html}
{description_html}
{field_rows}
<h2 class="t-h2">Use cases</h2>
{use_cases_html}
<h2 class="t-h2">Test coverage</h2>
{coverage_html}
<h2 class="t-h2">Interface</h2>
{interface_html}
<h2 class="t-h2">Version history</h2>
{version_history_html}
<details><summary>Full contract.json</summary>{raw_contract_block}</details>
<p style="margin-top:2rem"><a href="/#/capability/{encoded_reference}">Open in the interactive catalog →</a></p>
</div>
</body>
</html>
"""


def render_capability_page(base_url: str, entry: dict, group_versions: list) -> str:
    contract = entry["contract"]

    header_badges = [f'<span class="badge badge-accent">{esc(contract["namespace"])}</span>', f'<span class="badge">v{esc(contract["version"])}</span>']
    if contract.get("lifecycle"):
        header_badges.append(f'<span class="badge">{esc(contract["lifecycle"])}</span>')
    if entry["deprecated"]:
        header_badges.append('<span class="badge badge-danger">deprecated</span>')

    summary = contract.get("summary") or ""
    description = contract.get("description") or ""

    field_rows = "".join(
        [
            field_row("Service type", contract.get("service_type")),
            field_row("Permitted targets", ", ".join(contract.get("permitted_targets") or [])),
            field_row(
                "Owner",
                (contract.get("owner") or {}).get("team")
                and (contract["owner"]["team"] + (f" ({contract['owner']['contact']})" if contract["owner"].get("contact") else "")),
            ),
        ]
    )
    artifact = contract.get("artifact") or {}
    if artifact.get("digest"):
        field_rows += f'<div class="field-row"><span class="field-label">Artifact digest</span><span class="field-value t-mono">{esc(artifact["digest"])}</span></div>'
    if artifact.get("url"):
        field_rows += f'<div class="field-row"><span class="field-label">Artifact</span><span class="field-value"><a href="{esc(artifact["url"])}">{esc(artifact["url"])}</a></span></div>'

    use_cases = contract.get("use_cases") or []
    use_cases_html = "".join(use_case_block(uc) for uc in use_cases) if use_cases else '<p class="empty">No use cases published for this version yet.</p>'

    interface_parts = []
    if (contract.get("inputs") or {}).get("schema"):
        interface_parts.append('<div class="io-label">Input schema</div>' + json_block(contract["inputs"]["schema"]))
    if (contract.get("outputs") or {}).get("schema"):
        interface_parts.append('<div class="io-label" style="margin-top:0.75rem">Output schema</div>' + json_block(contract["outputs"]["schema"]))
    constraints = ((contract.get("execution") or {}).get("constraints")) or {}
    if constraints:
        interface_parts.append(field_row("Host API access", constraints.get("host_api_access")))
        interface_parts.append(field_row("Network access", constraints.get("network_access")))
        interface_parts.append(field_row("Filesystem access", constraints.get("filesystem_access")))
    permissions = contract.get("permissions") or []
    if permissions:
        interface_parts.append(field_row("Permissions", ", ".join(p.get("id", "") for p in permissions)))

    title = f"{contract['id']}@{contract['version']} · Traverse Registry Catalog"
    canonical_url = f"{base_url}/{capability_page_path(contract)}"

    return PAGE_TEMPLATE.format(
        title=esc(title),
        description=esc(summary or contract["id"]),
        canonical_url=esc(canonical_url),
        header_badges="".join(header_badges),
        id=esc(contract["id"]),
        summary_html=f'<p class="detail-summary">{esc(summary)}</p>' if summary else "",
        description_html=f'<p class="detail-description">{esc(description)}</p>' if description else "",
        field_rows=field_rows,
        use_cases_html=use_cases_html,
        coverage_html=coverage_block(entry.get("test_coverage")),
        interface_html="".join(interface_parts),
        version_history_html=version_history_block(base_url, group_versions, entry["reference"]),
        raw_contract_block=json_block(contract),
        encoded_reference=esc(entry["reference"]).replace("/", "%2F").replace("@", "%40"),
    )


def generate(catalog_path: Path, base_url: str, output_dir: Path) -> list:
    catalog = json.loads(catalog_path.read_text())
    capabilities = catalog.get("capabilities", [])

    by_group: dict = {}
    for entry in capabilities:
        group_key = f"{entry['contract']['namespace']}/{entry['contract']['id']}"
        by_group.setdefault(group_key, []).append(entry)

    generated_paths = []
    for entry in capabilities:
        group_key = f"{entry['contract']['namespace']}/{entry['contract']['id']}"
        page_html = render_capability_page(base_url, entry, by_group[group_key])
        page_path = output_dir / capability_page_path(entry["contract"]) / "index.html"
        page_path.parent.mkdir(parents=True, exist_ok=True)
        page_path.write_text(page_html)
        generated_paths.append(capability_page_path(entry["contract"]))

    return generated_paths


def write_sitemap(base_url: str, page_paths: list, output_path: Path) -> None:
    urls = [base_url + "/"] + [f"{base_url}/{path}" for path in sorted(page_paths)]
    entries = "\n".join(f"  <url><loc>{html.escape(url, quote=True)}</loc></url>" for url in urls)
    sitemap = f'<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n{entries}\n</urlset>\n'
    output_path.write_text(sitemap)


def main() -> int:
    if len(sys.argv) != 4:
        print("Usage: generate_catalog_pages.py <catalog.json> <base_url> <output_dir>", file=sys.stderr)
        return 1

    catalog_path = Path(sys.argv[1])
    base_url = sys.argv[2].rstrip("/")
    output_dir = Path(sys.argv[3])

    page_paths = generate(catalog_path, base_url, output_dir)
    write_sitemap(base_url, page_paths, output_dir / "sitemap.xml")

    print(f"Generated {len(page_paths)} static capability page(s) and sitemap.xml at {output_dir}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
