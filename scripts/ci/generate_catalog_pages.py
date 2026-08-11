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

Also generates one static page per persona (specs/017-persona-registry,
decision-log entry 53) at catalog/persona/<id>/<version>/index.html, with
its own full definition, distinguished_from cross-links, and a reverse
"used by" list of every capability use case that references it -- the same
crawlability reasoning as capability pages, and cross-linked from every
use case's persona badge on both the SPA and its own generated page.

Also generates one static page per service_type at
catalog/service-type/<id>/index.html. Unlike personas, service_type is a
closed, externally-defined enum (traverse-framework/traverse spec
014-service-type-taxonomy) this registry does not own or extend -- so
there's no governed content type or CI-enforced field here, just a fixed
SERVICE_TYPE_DEFINITIONS table and a reverse "capabilities" list per type,
cross-linked from each capability's own Service type field.

Usage: generate_catalog_pages.py <catalog.json> <base_url> <output_dir>
"""

import html
import json
import sys
from pathlib import Path
from typing import Optional
from urllib.parse import quote


def esc(value) -> str:
    return html.escape(str(value), quote=True)


def field_row(label: str, value) -> str:
    if not value:
        return ""
    return f'<div class="field-row"><span class="field-label">{esc(label)}</span><span class="field-value">{esc(value)}</span></div>'


def field_row_html(label: str, value_html: str) -> str:
    return f'<div class="field-row"><span class="field-label">{esc(label)}</span><span class="field-value">{value_html}</span></div>'


# service_type is a closed enum this registry does not own or extend --
# defined by traverse-framework/traverse's spec 014-service-type-taxonomy.
# Not a governed content type here (no personas/-style versioned records):
# just a fixed reference table for three values that will essentially never
# change, rendered as pages for discoverability/cross-linking.
SERVICE_TYPE_DEFINITIONS = {
    "stateless": {
        "name": "Stateless",
        "summary": "A pure function with no external state -- can execute on any target.",
        "description": "Every invocation is independent: given the same input, a stateless capability produces the same output, with no memory of prior calls and no managed persistence. Because it holds no state, it can run on any execution target (browser, edge, cloud, or device) -- the placement evaluator has no target-compatibility constraint to enforce. Today, every capability published in this registry is stateless.",
    },
    "subscribable": {
        "name": "Subscribable",
        "summary": "Event-activated -- runs when a declared event occurs, not on direct request.",
        "description": "Instead of being invoked directly, a subscribable capability declares the specific event that triggers it (event_trigger) and runs when that event occurs. No capability published in this registry declares this service type yet.",
    },
    "stateful": {
        "name": "Stateful",
        "summary": "Requires managed persistence across invocations -- target-constrained.",
        "description": "A stateful capability depends on durable state carried between invocations, which requires the runtime to provide managed persistence. Browser environments cannot guarantee durable storage, so a stateful capability can never declare Browser among its permitted targets -- the placement evaluator enforces this as a hard constraint, not just a recommendation. No capability published in this registry declares this service type yet.",
    },
}


def service_type_page_path(service_type_id: str) -> str:
    return f"service-type/{service_type_id}/"


def service_type_field_html(service_type: str, base_url: str) -> str:
    if not service_type:
        return ""
    definition = SERVICE_TYPE_DEFINITIONS.get(service_type)
    if not definition:
        return field_row("Service type", service_type)
    href = f"{base_url}/{service_type_page_path(service_type)}"
    return field_row_html("Service type", f'<a class="badge" href="{esc(href)}">{esc(definition["name"])}</a>')


# Never render contract.owner.contact directly -- it is free-form optional
# metadata (spec 006-public-scope-and-identity FR-003) and has held a
# personal email address for every capability published so far. Map the
# publishing team to a public GitHub identity instead.
OWNER_GITHUB = {"traverse-core": "enricopiovesan"}


def owner_html(owner: dict) -> str:
    team = (owner or {}).get("team")
    if not team:
        return ""
    handle = OWNER_GITHUB.get(team)
    if not handle:
        return field_row(team, team)
    link = f'<a href="https://github.com/{esc(handle)}" target="_blank" rel="noopener">@{esc(handle)}</a>'
    return field_row_html("Owner", f"{esc(team)} · {link}")


def find_event_by_id(catalog_events: list, event_id: str) -> Optional[dict]:
    matches = [
        entry
        for entry in catalog_events
        if (entry.get("product") or {}).get("contract", {}).get("id") == event_id
    ]
    if not matches:
        return None
    matches.sort(
        key=lambda entry: tuple(
            int(part)
            for part in str(entry["product"]["contract"]["version"]).split(".")
            if part.isdigit()
        )
    )
    return matches[-1]


def events_html(contract: dict, catalog_events: list) -> str:
    publishes = contract.get("emits") or []
    consumes = contract.get("consumes") or []
    if not publishes and not consumes:
        return (
            '<p class="empty">This capability does not declare any '
            "published or consumed events.</p>"
        )

    def event_list(label: str, items: list) -> str:
        if not items:
            return field_row_html(label, '<span class="t-muted">none</span>')
        links = []
        for item in items:
            event_id = item if isinstance(item, str) else (item or {}).get("event_id") or (item or {}).get("id")
            published = find_event_by_id(catalog_events, event_id) if event_id else None
            if published:
                version = published["product"]["contract"].get("version", "")
                href = f"/#/event/{quote(published['reference'], safe='')}"
                version_suffix = f" @{esc(version)}" if version else ""
                links.append(
                    f'<div><a href="{esc(href)}">{esc(event_id)}</a>{version_suffix}</div>'
                )
            else:
                links.append(f'<div class="t-mono">{esc(json.dumps(item))}</div>')
        return field_row_html(label, "".join(links))

    return event_list("Publishes", publishes) + event_list("Consumes", consumes)


def json_block(value) -> str:
    return f'<div class="code-block"><pre>{esc(json.dumps(value, indent=2))}</pre></div>'


def redacted_contract(contract: dict) -> dict:
    redacted = dict(contract)
    owner = redacted.get("owner")
    if owner and owner.get("contact"):
        redacted["owner"] = dict(owner, contact="[redacted -- see Owner field above]")
    return redacted


def persona_page_path(persona: dict) -> str:
    return f"persona/{persona['id']}/{persona['version']}/"


def use_case_block(use_case: dict, base_url: str, personas_by_id: dict) -> str:
    happy = use_case.get("happy", True) is not False
    css_class = "use-case" if happy else "use-case unhappy"
    persona_html = ""
    persona_ref = use_case.get("persona_ref")
    if persona_ref:
        persona_entry = personas_by_id.get(persona_ref)
        if persona_entry:
            href = f"{base_url}/{persona_page_path(persona_entry['persona'])}"
            persona_html = f'<div class="use-case-persona">Persona: <a class="badge" href="{esc(href)}">{esc(persona_entry["persona"]["name"])}</a></div>'
        else:
            persona_html = f'<div class="use-case-persona">Persona: <span class="badge badge-danger">{esc(persona_ref)} (unregistered)</span></div>'
    return (
        f'<div class="{css_class}">'
        f'<p class="use-case-scenario">{esc(use_case.get("scenario", ""))}</p>'
        f"{persona_html}"
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
<script defer src="/analytics.js"></script>
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
<h2 class="t-h2">Events</h2>
{events_html}
<h2 class="t-h2">Version history</h2>
{version_history_html}
<details><summary>Full contract.json</summary>{raw_contract_block}</details>
<p style="margin-top:2rem"><a href="/#/capability/{encoded_reference}">Open in the interactive catalog →</a></p>
</div>
</body>
</html>
"""


def render_capability_page(
    base_url: str,
    entry: dict,
    group_versions: list,
    personas_by_id: dict,
    catalog_events: Optional[list] = None,
) -> str:
    contract = entry["contract"]
    catalog_events = catalog_events or []

    header_badges = [f'<span class="badge badge-accent">{esc(contract["namespace"])}</span>', f'<span class="badge">v{esc(contract["version"])}</span>']
    if contract.get("lifecycle"):
        header_badges.append(f'<span class="badge">{esc(contract["lifecycle"])}</span>')
    if entry["deprecated"]:
        header_badges.append('<span class="badge badge-danger">deprecated</span>')

    summary = contract.get("summary") or ""
    description = contract.get("description") or ""

    field_rows = "".join(
        [
            service_type_field_html(contract.get("service_type"), base_url),
            field_row("Permitted targets", ", ".join(contract.get("permitted_targets") or [])),
            owner_html(contract.get("owner")),
        ]
    )
    artifact = contract.get("artifact") or {}
    if artifact.get("digest"):
        field_rows += f'<div class="field-row"><span class="field-label">Artifact digest</span><span class="field-value t-mono">{esc(artifact["digest"])}</span></div>'
    if artifact.get("url"):
        field_rows += f'<div class="field-row"><span class="field-label">Artifact</span><span class="field-value"><a href="{esc(artifact["url"])}">{esc(artifact["url"])}</a></span></div>'

    use_cases = contract.get("use_cases") or []
    use_cases_html = (
        "".join(use_case_block(uc, base_url, personas_by_id) for uc in use_cases)
        if use_cases
        else '<p class="empty">No use cases published for this version yet.</p>'
    )

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
        events_html=events_html(contract, catalog_events),
        version_history_html=version_history_block(base_url, group_versions, entry["reference"]),
        raw_contract_block=json_block(redacted_contract(contract)),
        encoded_reference=esc(entry["reference"]).replace("/", "%2F").replace("@", "%40"),
    )


PERSONA_PAGE_TEMPLATE = """<!doctype html>
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
<script defer src="/analytics.js"></script>
</head>
<body>
<nav class="nav">
  <div class="nav-inner">
    <a href="/" class="nav-logo"><span>Traverse<span class="nav-logo-dot">.</span></span><span class="nav-label">REGISTRY CATALOG</span></a>
  </div>
</nav>
<div class="container">
<a class="back-link" href="/#/personas">← Back to personas</a>
<div class="detail-header-badges"><span class="badge">v{version}</span></div>
<h1 class="t-h1 detail-title">{name}</h1>
{summary_html}
{description_html}
<h2 class="t-h2">Distinguished from</h2>
{distinguished_from_html}
<h2 class="t-h2">Used by</h2>
{used_by_html}
<p style="margin-top:2rem"><a href="/#/persona/{encoded_reference}">Open in the interactive catalog →</a></p>
</div>
</body>
</html>
"""


def render_persona_page(base_url: str, persona_entry: dict, personas_by_id: dict, capabilities: list) -> str:
    persona = persona_entry["persona"]

    distinguished_from = persona.get("distinguished_from") or []
    if distinguished_from:
        rows = []
        for item in distinguished_from:
            other = personas_by_id.get(item.get("persona_id"))
            if other:
                href = f"{base_url}/{persona_page_path(other['persona'])}"
                label_html = f'<a href="{esc(href)}">{esc(other["persona"]["name"])}</a>'
            else:
                label_html = f'<span class="badge badge-danger">{esc(item.get("persona_id"))} (unregistered)</span>'
            rows.append(field_row_html(other["persona"]["name"] if other else item.get("persona_id"), f'{label_html} -- {esc(item.get("how"))}'))
        distinguished_from_html = "".join(rows)
    else:
        distinguished_from_html = '<p class="empty">No other personas registered yet.</p>'

    uses = []
    for entry in capabilities:
        for use_case in entry["contract"].get("use_cases") or []:
            if use_case.get("persona_ref") == persona["id"]:
                uses.append((entry, use_case))

    if uses:
        rows = []
        for entry, use_case in uses:
            href = f"{base_url}/{capability_page_path(entry['contract'])}"
            happy = use_case.get("happy", True) is not False
            badge = '<span class="badge">happy</span>' if happy else '<span class="badge badge-danger">unhappy</span>'
            rows.append(
                f'<a class="version-row" href="{esc(href)}"><div class="version-left">'
                f'<span class="t-mono">{esc(entry["contract"]["id"])}</span> {badge}</div>'
                f'<span class="t-muted" style="font-size:0.8rem">view →</span></a>'
            )
        used_by_html = "\n".join(rows)
    else:
        used_by_html = '<p class="empty">No published use case references this persona yet.</p>'

    title = f"{persona['name']} · Traverse Registry Catalog"
    canonical_url = f"{base_url}/{persona_page_path(persona)}"

    return PERSONA_PAGE_TEMPLATE.format(
        title=esc(title),
        description=esc(persona.get("summary") or persona["name"]),
        canonical_url=esc(canonical_url),
        version=esc(persona["version"]),
        name=esc(persona["name"]),
        summary_html=f'<p class="detail-summary">{esc(persona["summary"])}</p>' if persona.get("summary") else "",
        description_html=f'<p class="detail-description">{esc(persona["description"])}</p>' if persona.get("description") else "",
        distinguished_from_html=distinguished_from_html,
        used_by_html=used_by_html,
        encoded_reference=esc(persona_entry["reference"]).replace("/", "%2F").replace("@", "%40"),
    )


def current_personas_by_id(personas: list) -> dict:
    """One entry per persona id, pointing at its highest version -- same
    convention capabilities use for their own 'current' version."""
    by_id: dict = {}
    for entry in personas:
        pid = entry["persona"]["id"]
        current = by_id.get(pid)
        if current is None or semver_key(entry["persona"]["version"]) > semver_key(current["persona"]["version"]):
            by_id[pid] = entry
    return by_id


SERVICE_TYPE_PAGE_TEMPLATE = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<meta name="description" content="{meta_description}">
<meta property="og:type" content="website">
<meta property="og:title" content="{title}">
<meta property="og:description" content="{meta_description}">
<meta property="og:url" content="{canonical_url}">
<link rel="canonical" href="{canonical_url}">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Space+Grotesk:wght@500;600;700&family=Inter:wght@300;400;500;600&family=JetBrains+Mono:wght@400;500&display=swap" rel="stylesheet">
<link rel="stylesheet" href="/style.css">
<script defer src="/analytics.js"></script>
</head>
<body>
<nav class="nav">
  <div class="nav-inner">
    <a href="/" class="nav-logo"><span>Traverse<span class="nav-logo-dot">.</span></span><span class="nav-label">REGISTRY CATALOG</span></a>
  </div>
</nav>
<div class="container">
<a class="back-link" href="/#/service-types">← Back to service types</a>
<h1 class="t-h1 detail-title">{name}</h1>
<p class="detail-summary">{summary}</p>
<p class="detail-description">{description}</p>
<h2 class="t-h2">Capabilities</h2>
{capabilities_html}
<p style="margin-top:2rem"><a href="/#/service-type/{id}">Open in the interactive catalog →</a></p>
</div>
</body>
</html>
"""


def current_capabilities_by_group(capabilities: list) -> list:
    """One entry per distinct namespace/id, pointing at its highest
    version -- same 'current' convention as current_personas_by_id, needed
    here so a service-type page doesn't list every historical version of
    a capability that has been republished multiple times."""
    by_group: dict = {}
    for entry in capabilities:
        group_key = f"{entry['contract']['namespace']}/{entry['contract']['id']}"
        current = by_group.get(group_key)
        if current is None or semver_key(entry["contract"]["version"]) > semver_key(current["contract"]["version"]):
            by_group[group_key] = entry
    return list(by_group.values())


def render_service_type_page(base_url: str, service_type_id: str, definition: dict, capabilities: list) -> str:
    current_capabilities = current_capabilities_by_group(capabilities)
    matching = [entry for entry in current_capabilities if entry["contract"].get("service_type") == service_type_id]

    if matching:
        rows = []
        for entry in matching:
            href = f"{base_url}/{capability_page_path(entry['contract'])}"
            rows.append(
                f'<a class="version-row" href="{esc(href)}"><div class="version-left">'
                f'<span class="t-mono">{esc(entry["contract"]["id"])}</span></div>'
                f'<span class="t-muted" style="font-size:0.8rem">view →</span></a>'
            )
        capabilities_html = "\n".join(rows)
    else:
        capabilities_html = '<p class="empty">No published capability declares this service type yet.</p>'

    title = f"{definition['name']} · Traverse Registry Catalog"
    canonical_url = f"{base_url}/{service_type_page_path(service_type_id)}"

    return SERVICE_TYPE_PAGE_TEMPLATE.format(
        title=esc(title),
        meta_description=esc(definition["summary"]),
        canonical_url=esc(canonical_url),
        name=esc(definition["name"]),
        summary=esc(definition["summary"]),
        description=esc(definition["description"]),
        capabilities_html=capabilities_html,
        id=esc(service_type_id),
    )


def generate(catalog_path: Path, base_url: str, output_dir: Path) -> list:
    catalog = json.loads(catalog_path.read_text())
    capabilities = catalog.get("capabilities", [])
    personas = catalog.get("personas", [])
    catalog_events = catalog.get("events", [])
    personas_by_id = current_personas_by_id(personas)

    by_group: dict = {}
    for entry in capabilities:
        group_key = f"{entry['contract']['namespace']}/{entry['contract']['id']}"
        by_group.setdefault(group_key, []).append(entry)

    generated_paths = []
    for entry in capabilities:
        group_key = f"{entry['contract']['namespace']}/{entry['contract']['id']}"
        page_html = render_capability_page(
            base_url, entry, by_group[group_key], personas_by_id, catalog_events
        )
        page_path = output_dir / capability_page_path(entry["contract"]) / "index.html"
        page_path.parent.mkdir(parents=True, exist_ok=True)
        page_path.write_text(page_html)
        generated_paths.append(capability_page_path(entry["contract"]))

    for persona_entry in personas_by_id.values():
        page_html = render_persona_page(base_url, persona_entry, personas_by_id, capabilities)
        page_path = output_dir / persona_page_path(persona_entry["persona"]) / "index.html"
        page_path.parent.mkdir(parents=True, exist_ok=True)
        page_path.write_text(page_html)
        generated_paths.append(persona_page_path(persona_entry["persona"]))

    for service_type_id, definition in SERVICE_TYPE_DEFINITIONS.items():
        page_html = render_service_type_page(base_url, service_type_id, definition, capabilities)
        page_path = output_dir / service_type_page_path(service_type_id) / "index.html"
        page_path.parent.mkdir(parents=True, exist_ok=True)
        page_path.write_text(page_html)
        generated_paths.append(service_type_page_path(service_type_id))

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

    print(f"Generated {len(page_paths)} static page(s) (capabilities + personas) and sitemap.xml at {output_dir}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
