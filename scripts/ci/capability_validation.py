#!/usr/bin/env python3
"""Deterministic capability validation gate.

Implements specs/002-capability-validation/spec.md FR-001 through FR-005.
Walks capabilities/**/contract.json, validates schema/path/semver, checks
namespace-collision-safe immutability (no PR may modify an existing
contract.json), and checks yank records (specs/005-yank-deprecation)
never accompany a modified contract.json. Also enforces the finalized
owner/namespace/scope field shapes from specs/006-public-scope-and-identity
FR-002 through FR-004.

Also enforces specs/001-registry-foundation FR-011 (amended, decision-log
entry 46, registry#140): a newly-published use_cases[].scenario must be a
full user story, not a plain declarative sentence. Diff-based (only checks
contract.json files newly ADDED in a PR, via git diff) rather than run
against the whole historical tree -- older, already-published, immutable
versions predate this requirement and can never be edited to match it, so
checking them unconditionally would fail permanently and forever. See
check_new_scenario_format's docstring for the concrete incident that
confirmed this.

Also enforces that a contract's service_type, if present, is one of the
three values traverse-framework/traverse's spec 014-service-type-taxonomy
defines (stateless/subscribable/stateful) -- a closed enum registry does
not own, validated whole-tree since every already-published contract
already conforms.

Also enforces specs/017-persona-registry (decision-log entry 53): every
personas/<id>/<version>/persona.json must carry the required fields
(including a non-empty distinguished_from list, once more than one persona
is registered), every distinguished_from reference must resolve to a real
persona id, and every use_cases[] entry in a newly-ADDED contract.json must
carry a persona_ref resolving to a real, registered persona. Persona shape
and distinguished_from resolution run unconditionally (whole-tree) since
that schema was correct from this spec's very first persona -- only the
persona_ref-on-use_cases requirement is diff-based, for the same reason the
scenario-format check is: older contract.json versions predate the field
and can never be edited to add it.

Also enforces traverse-framework/traverse Spec 102-contract-surface-coverage
FR-001 (registry#192): when a newly-ADDED contract.json declares
inputs.schema.properties.action.enum, every enum value must appear as
use_cases[].input_example.action for at least one use case. Diff-based for
the same immutability reason as scenario/persona_ref checks.

Also enforces specs/001-registry-foundation FR-007 and
specs/007-artifact-hosting FR-001 (registry#187): a newly-ADDED
contract.json MUST include artifact.digest (sha256:…) and artifact.url
pointing at this repo's artifacts/<tag>/<asset> GitHub Release download
URL. Diff-based so already-published immutable versions that predate (or
were broken by) this requirement are never re-judged -- the same reason
scenario/persona_ref checks are diff-based. The index builder already
hard-fails active contracts missing these fields; this gate blocks the
unusable publish at PR time instead.
"""

import json
import re
import subprocess
import sys
from pathlib import Path

SEMVER_RE = re.compile(
    r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)"
    r"(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)"
    r"(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?"
    r"(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$"
)

# specs/007-artifact-hosting: assets live under this repo's
# artifacts/<id>-<version> (or artifacts/<namespace>.<id>-<version>)
# GitHub Release tags. Allowed host is this registry only -- consumers
# must not depend on another repo's release hygiene for immutability.
ARTIFACT_RELEASE_URL_RE = re.compile(
    r"^https://github\.com/traverse-framework/registry/releases/download/"
    r"artifacts/[^/]+/[^/]+$"
)

REQUIRED_FIELDS = ["id", "namespace", "owner", "version"]

# traverse-framework/traverse spec 014-service-type-taxonomy (external,
# authoritative -- this is a closed enum registry does not own or extend).
# Safe to validate whole-tree, unlike use_cases[].scenario/persona_ref:
# every one of the 61 already-published contracts (current and historical)
# already uses one of these three values, confirmed by inspection before
# adding this check, so there is no legacy-incompatibility risk.
KNOWN_SERVICE_TYPES = {"stateless", "subscribable", "stateful"}


def fail(errors, code, path, message):
    errors.append({"code": code, "path": path, "message": message})


def is_user_story_scenario(scenario) -> bool:
    """spec 001 FR-011 (amended, decision-log entry 46): a use_cases[]
    scenario must be a full user story -- "As a <persona>, I want to
    <action>, so that <benefit>." -- not a plain declarative sentence.
    Deliberately permissive (substring presence + order, not a rigid
    regex): real scenario prose varies in wording/punctuation around these
    three clauses, and a strict pattern would false-positive on a
    legitimately-phrased story."""
    if not isinstance(scenario, str):
        return False
    lowered = scenario.lower()
    as_a_index = lowered.find("as a")
    if as_a_index == -1:
        return False
    i_want_index = lowered.find("i want", as_a_index)
    if i_want_index == -1:
        return False
    so_that_index = lowered.find("so that", i_want_index)
    return so_that_index != -1


def validate_contract(path: Path, errors: list) -> None:
    try:
        contract = json.loads(path.read_text())
    except Exception as exc:
        fail(errors, "contract.invalid_json", str(path), f"Unable to parse JSON: {exc}")
        return

    for field in REQUIRED_FIELDS:
        if field not in contract:
            fail(
                errors,
                "contract.missing_required_field",
                str(path),
                f"Missing required field '{field}'",
            )

    # path is capabilities/<namespace>/<id>/<version>/contract.json
    parts = path.parts
    try:
        idx = parts.index("capabilities")
        namespace_seg, id_seg, version_seg = parts[idx + 1], parts[idx + 2], parts[idx + 3]
    except (ValueError, IndexError):
        fail(errors, "contract.bad_path", str(path), "Path does not match capabilities/<namespace>/<id>/<version>/contract.json")
        return

    namespace = contract.get("namespace")
    if namespace is not None and (not isinstance(namespace, str) or not namespace.strip()):
        fail(
            errors,
            "contract.invalid_namespace",
            str(path),
            "namespace must be a non-empty string (spec 006 FR-002)",
        )
    elif namespace and namespace != namespace_seg:
        fail(
            errors,
            "contract.namespace_mismatch",
            str(path),
            f"contract.json namespace '{namespace}' does not match path segment '{namespace_seg}'",
        )

    owner = contract.get("owner")
    if owner is not None and (
        not isinstance(owner, dict) or not isinstance(owner.get("team"), str) or not owner.get("team").strip()
    ):
        fail(
            errors,
            "contract.invalid_owner",
            str(path),
            "owner must be an object with a non-empty 'team' string (spec 006 FR-003)",
        )

    if "scope" in contract:
        fail(
            errors,
            "contract.forbidden_scope_field",
            str(path),
            "contract.json must not declare a top-level 'scope' field -- resolution tier is a "
            "consumer-side concept, not part of a published record (spec 006 FR-004)",
        )

    service_type = contract.get("service_type")
    if service_type is not None and service_type not in KNOWN_SERVICE_TYPES:
        fail(
            errors,
            "contract.invalid_service_type",
            str(path),
            f"service_type '{service_type}' is not one of {sorted(KNOWN_SERVICE_TYPES)} "
            "(traverse-framework/traverse spec 014-service-type-taxonomy)",
        )

    if contract.get("id") and contract.get("id") != id_seg:
        fail(
            errors,
            "contract.id_mismatch",
            str(path),
            f"contract.json id '{contract.get('id')}' does not match path segment '{id_seg}'",
        )

    version = contract.get("version")
    if version and version != version_seg:
        fail(
            errors,
            "contract.version_mismatch",
            str(path),
            f"contract.json version '{version}' does not match path segment '{version_seg}'",
        )

    if version and not SEMVER_RE.match(version):
        fail(errors, "contract.invalid_semver", str(path), f"'{version}' is not a valid semver string")

    artifact = contract.get("artifact")
    if artifact is not None:
        if not isinstance(artifact, dict) or "digest" not in artifact or "url" not in artifact:
            fail(
                errors,
                "contract.invalid_artifact_reference",
                str(path),
                "artifact reference must include 'digest' and 'url'",
            )
        elif not str(artifact["digest"]).startswith("sha256:"):
            fail(errors, "contract.invalid_digest_format", str(path), "artifact digest must be a 'sha256:' prefixed value")


def check_new_scenario_format(path: Path, errors: list) -> None:
    """spec 001 FR-011 (amended, decision-log entry 46): a use_cases[]
    scenario must be a full user story. Deliberately NOT called from
    validate_contract, which runs unconditionally on every contract.json in
    the tree, including every already-published, immutable older version --
    those can never be edited to match a format introduced after they were
    published, so checking them here would fail permanently and forever
    (confirmed empirically: running this against the full pre-#139 tree
    failed on 20 historical versions with no way to ever fix them). Wired
    into main() as a diff-based check instead, the same way
    check_immutability only looks at what a PR actually adds -- see
    check_new_scenarios_are_user_stories below."""
    try:
        contract = json.loads(path.read_text())
    except Exception:
        return
    use_cases = contract.get("use_cases")
    if not isinstance(use_cases, list):
        return
    for index, use_case in enumerate(use_cases):
        scenario = use_case.get("scenario") if isinstance(use_case, dict) else None
        if not is_user_story_scenario(scenario):
            fail(
                errors,
                "contract.scenario_not_user_story",
                str(path),
                f"use_cases[{index}].scenario must be a full user story "
                "('As a <persona>, I want to <action>, so that <benefit>.'), "
                f"not a plain declarative sentence (spec 001 FR-011): {scenario!r}",
            )


def check_new_scenarios_are_user_stories(base_sha: str, head_sha: str, errors: list) -> None:
    """Only validates newly-ADDED contract.json files in this PR's diff --
    see check_new_scenario_format's docstring for why this must not run
    against the whole historical tree."""
    diff = subprocess.check_output(
        ["git", "diff", "--name-status", f"{base_sha}...{head_sha}", "--", "capabilities/"],
        text=True,
    )
    for line in diff.splitlines():
        if not line.strip():
            continue
        parts = line.split("\t")
        status, path = parts[0], parts[-1]
        if status == "A" and path.endswith("contract.json"):
            check_new_scenario_format(Path(path), errors)


PERSONA_REQUIRED_FIELDS = ["id", "version", "name", "summary", "description", "distinguished_from"]


def validate_persona(path: Path, errors: list) -> None:
    """spec 017-persona-registry FR-001/FR-002/FR-003: a persona.json must
    carry the required fields, its id/version must match its path, and its
    distinguished_from entries must be well-formed. Runs unconditionally
    (whole-tree, unlike the diff-based use_cases[].persona_ref check) --
    this schema is correct for every persona from the spec's first version,
    so there is no historical-content problem to guard against here."""
    try:
        persona = json.loads(path.read_text())
    except Exception as exc:
        fail(errors, "persona.invalid_json", str(path), f"Unable to parse JSON: {exc}")
        return

    for field in PERSONA_REQUIRED_FIELDS:
        if field not in persona:
            fail(errors, "persona.missing_required_field", str(path), f"Missing required field '{field}'")

    # path is personas/<persona-id>/<version>/persona.json
    parts = path.parts
    try:
        idx = parts.index("personas")
        id_seg, version_seg = parts[idx + 1], parts[idx + 2]
    except (ValueError, IndexError):
        fail(errors, "persona.bad_path", str(path), "Path does not match personas/<persona-id>/<version>/persona.json")
        return

    if persona.get("id") and persona.get("id") != id_seg:
        fail(errors, "persona.id_mismatch", str(path), f"persona.json id '{persona.get('id')}' does not match path segment '{id_seg}'")

    version = persona.get("version")
    if version and version != version_seg:
        fail(errors, "persona.version_mismatch", str(path), f"persona.json version '{version}' does not match path segment '{version_seg}'")
    if version and not SEMVER_RE.match(version):
        fail(errors, "persona.invalid_semver", str(path), f"'{version}' is not a valid semver string")

    distinguished_from = persona.get("distinguished_from")
    if distinguished_from is not None:
        if not isinstance(distinguished_from, list):
            fail(errors, "persona.invalid_distinguished_from", str(path), "distinguished_from must be an array")
        else:
            for index, entry in enumerate(distinguished_from):
                if not isinstance(entry, dict) or not entry.get("persona_id") or not entry.get("how"):
                    fail(
                        errors,
                        "persona.invalid_distinguished_from_entry",
                        str(path),
                        f"distinguished_from[{index}] must be an object with non-empty 'persona_id' and 'how'",
                    )


def collect_registered_persona_ids(personas_dir: Path) -> set:
    ids = set()
    for persona_path in sorted(personas_dir.rglob("persona.json")):
        try:
            persona = json.loads(persona_path.read_text())
        except Exception:
            continue
        if persona.get("id"):
            ids.add(persona["id"])
    return ids


def check_persona_distinguished_from_resolves(errors: list) -> None:
    """spec 017-persona-registry FR-002/FR-006: distinguished_from must be
    non-empty whenever another persona is registered, and every reference
    in it must resolve to a real persona id."""
    personas_dir = Path("personas")
    if not personas_dir.is_dir():
        return

    persona_ids = collect_registered_persona_ids(personas_dir)

    for persona_path in sorted(personas_dir.rglob("persona.json")):
        try:
            persona = json.loads(persona_path.read_text())
        except Exception:
            continue
        own_id = persona.get("id")
        distinguished_from = persona.get("distinguished_from")
        if not isinstance(distinguished_from, list):
            continue

        if not distinguished_from and len(persona_ids) > 1:
            fail(
                errors,
                "persona.empty_distinguished_from",
                str(persona_path),
                f"distinguished_from must be non-empty -- {len(persona_ids) - 1} other persona(s) are registered",
            )

        for entry in distinguished_from:
            if not isinstance(entry, dict):
                continue
            ref = entry.get("persona_id")
            if ref and ref not in persona_ids:
                fail(
                    errors,
                    "persona.distinguished_from_unresolvable",
                    str(persona_path),
                    f"distinguished_from references unknown persona id '{ref}'",
                )
            if ref == own_id:
                fail(
                    errors,
                    "persona.distinguished_from_self_reference",
                    str(persona_path),
                    "distinguished_from must not reference its own persona id",
                )


def check_new_use_case_persona_ref(path: Path, errors: list, persona_ids: set) -> None:
    """spec 017-persona-registry FR-004/FR-005: a newly-added contract.json's
    use_cases[] entries must each carry a persona_ref resolving to a real,
    registered persona. Deliberately NOT called from validate_contract for
    the same reason check_new_scenario_format isn't: already-published,
    immutable older versions predate this field and can never be edited to
    add it."""
    try:
        contract = json.loads(path.read_text())
    except Exception:
        return
    use_cases = contract.get("use_cases")
    if not isinstance(use_cases, list):
        return
    for index, use_case in enumerate(use_cases):
        persona_ref = use_case.get("persona_ref") if isinstance(use_case, dict) else None
        if not persona_ref:
            fail(
                errors,
                "contract.use_case_missing_persona_ref",
                str(path),
                f"use_cases[{index}] is missing persona_ref (spec 017-persona-registry FR-004)",
            )
        elif persona_ref not in persona_ids:
            fail(
                errors,
                "contract.use_case_persona_ref_unresolvable",
                str(path),
                f"use_cases[{index}].persona_ref '{persona_ref}' does not resolve to a registered persona (spec 017-persona-registry FR-005)",
            )


def check_new_use_cases_have_persona_ref(base_sha: str, head_sha: str, errors: list) -> None:
    """Only validates newly-ADDED contract.json files in this PR's diff --
    see check_new_use_case_persona_ref's docstring for why this must not run
    against the whole historical tree."""
    persona_ids = collect_registered_persona_ids(Path("personas"))
    diff = subprocess.check_output(
        ["git", "diff", "--name-status", f"{base_sha}...{head_sha}", "--", "capabilities/"],
        text=True,
    )
    for line in diff.splitlines():
        if not line.strip():
            continue
        parts = line.split("\t")
        status, path = parts[0], parts[-1]
        if status == "A" and path.endswith("contract.json"):
            check_new_use_case_persona_ref(Path(path), errors, persona_ids)


def check_new_action_enum_covered_by_use_cases(path: Path, errors: list) -> None:
    """traverse Spec 102-contract-surface-coverage FR-001 / registry#192:
    every inputs.schema.properties.action.enum value on a newly-added
    contract.json must appear in some use_cases[].input_example.action.
    Skip when action.enum is absent (capabilities without that discriminator).
    """
    try:
        contract = json.loads(path.read_text())
    except Exception:
        return
    action_schema = (
        contract.get("inputs", {})
        .get("schema", {})
        .get("properties", {})
        .get("action")
    )
    if not isinstance(action_schema, dict):
        return
    enum_values = action_schema.get("enum")
    if not isinstance(enum_values, list) or not enum_values:
        return
    declared = []
    for value in enum_values:
        if not isinstance(value, str):
            fail(
                errors,
                "contract.action_enum_non_string",
                str(path),
                "inputs.schema.properties.action.enum must contain only strings (spec 102 FR-001)",
            )
            return
        declared.append(value)
    use_cases = contract.get("use_cases")
    covered = set()
    if isinstance(use_cases, list):
        for use_case in use_cases:
            if not isinstance(use_case, dict):
                continue
            input_example = use_case.get("input_example")
            if isinstance(input_example, dict):
                action = input_example.get("action")
                if isinstance(action, str):
                    covered.add(action)
    uncovered = [action for action in declared if action not in covered]
    if uncovered:
        fail(
            errors,
            "contract.action_enum_uncovered_by_use_cases",
            str(path),
            "inputs.schema.properties.action.enum values lack covering use_cases: "
            + ", ".join(uncovered)
            + " (traverse Spec 102-contract-surface-coverage FR-001)",
        )


def check_new_contracts_action_enum_coverage(base_sha: str, head_sha: str, errors: list) -> None:
    """Only validates newly-ADDED contract.json files in this PR's diff --
    older immutable publishes may predate Spec 102 coverage."""
    diff = subprocess.check_output(
        ["git", "diff", "--name-status", f"{base_sha}...{head_sha}", "--", "capabilities/"],
        text=True,
    )
    for line in diff.splitlines():
        if not line.strip():
            continue
        parts = line.split("\t")
        status, path = parts[0], parts[-1]
        if status == "A" and path.endswith("contract.json"):
            check_new_action_enum_covered_by_use_cases(Path(path), errors)


def check_new_contract_artifact_reference(path: Path, errors: list) -> None:
    """spec 001 FR-007 / spec 007 FR-001 / registry#187: a newly-added
    contract.json must carry a fetchable artifact reference. Deliberately
    NOT called from validate_contract (whole-tree): two already-published
    deprecated versions lack artifact after traverse-cli stripped the field
    (traverse#859), and contracts are immutable, so checking the historical
    tree would fail permanently. Wired into main() as a diff-based check.
    """
    try:
        contract = json.loads(path.read_text())
    except Exception:
        return
    artifact = contract.get("artifact")
    if not isinstance(artifact, dict):
        fail(
            errors,
            "contract.missing_artifact_reference",
            str(path),
            "newly-added contract.json must include artifact.digest and "
            "artifact.url (spec 001 FR-007 / spec 007 FR-001); upload the "
            "WASM under an artifacts/<id>-<version> release before opening "
            "the contract PR (see traverse-framework/traverse#859 for the "
            "CLI publish path that historically dropped these fields)",
        )
        return
    digest = artifact.get("digest")
    url = artifact.get("url")
    if not digest or not url:
        fail(
            errors,
            "contract.missing_artifact_reference",
            str(path),
            "newly-added contract.json artifact must include both "
            "'digest' and 'url' (spec 001 FR-007 / spec 007 FR-001)",
        )
        return
    if not str(digest).startswith("sha256:"):
        fail(
            errors,
            "contract.invalid_digest_format",
            str(path),
            "artifact digest must be a 'sha256:' prefixed value",
        )
    if not ARTIFACT_RELEASE_URL_RE.match(str(url)):
        fail(
            errors,
            "contract.invalid_artifact_url",
            str(path),
            "artifact.url must be a GitHub Release asset under "
            "https://github.com/traverse-framework/registry/releases/download/"
            "artifacts/<tag>/<asset> (spec 007-artifact-hosting)",
        )


def check_new_contracts_have_artifact_reference(base_sha: str, head_sha: str, errors: list) -> None:
    """Only validates newly-ADDED contract.json files in this PR's diff --
    see check_new_contract_artifact_reference's docstring for why this must
    not run against the whole historical tree."""
    diff = subprocess.check_output(
        ["git", "diff", "--name-status", f"{base_sha}...{head_sha}", "--", "capabilities/"],
        text=True,
    )
    for line in diff.splitlines():
        if not line.strip():
            continue
        parts = line.split("\t")
        status, path = parts[0], parts[-1]
        if status == "A" and path.endswith("contract.json"):
            check_new_contract_artifact_reference(Path(path), errors)


def real_workflow_paths(workflows_dir: Path):
    """Real, published workflow.json files only -- excludes
    workflows/examples/, which holds demo/fixture content
    (workflows/examples/expedition/plan-expedition/) that predates FR-013's
    real workflows/<namespace>/<id>/<version>/ layout and doesn't follow it,
    the same way capability_validation.py never walks examples/applications/."""
    return sorted(p for p in workflows_dir.rglob("workflow.json") if "examples" not in p.parts)


def validate_workflow(path: Path, errors: list) -> None:
    """Workflow records are governed the same way capability records are
    (spec 001 FR-013, referencing FR-001/FR-002): immutable versioned
    directories, path-consistent identity, valid semver. Not every
    capability-specific check applies -- a workflow.json has no `artifact`
    or forbidden `scope` field -- so this validates the structural subset
    that genuinely carries over, not a full duplicate of validate_contract.
    """
    try:
        workflow = json.loads(path.read_text())
    except Exception as exc:
        fail(errors, "workflow.invalid_json", str(path), f"Unable to parse JSON: {exc}")
        return

    for field in ["id", "namespace", "owner", "version", "nodes", "edges", "start_node", "terminal_nodes"]:
        if field not in workflow:
            fail(errors, "workflow.missing_required_field", str(path), f"Missing required field '{field}'")

    # path is workflows/<namespace>/<id>/<version>/workflow.json
    parts = path.parts
    try:
        idx = parts.index("workflows")
        namespace_seg, id_seg, version_seg = parts[idx + 1], parts[idx + 2], parts[idx + 3]
    except (ValueError, IndexError):
        fail(errors, "workflow.bad_path", str(path), "Path does not match workflows/<namespace>/<id>/<version>/workflow.json")
        return

    namespace = workflow.get("namespace")
    if namespace and namespace != namespace_seg:
        fail(
            errors,
            "workflow.namespace_mismatch",
            str(path),
            f"workflow.json namespace '{namespace}' does not match path segment '{namespace_seg}'",
        )

    if workflow.get("id") and workflow.get("id") != id_seg:
        fail(errors, "workflow.id_mismatch", str(path), f"workflow.json id '{workflow.get('id')}' does not match path segment '{id_seg}'")

    version = workflow.get("version")
    if version and version != version_seg:
        fail(errors, "workflow.version_mismatch", str(path), f"workflow.json version '{version}' does not match path segment '{version_seg}'")
    if version and not SEMVER_RE.match(version):
        fail(errors, "workflow.invalid_semver", str(path), f"'{version}' is not a valid semver string")


def check_workflow_capability_references(errors: list) -> None:
    """A workflow's nodes must reference capability versions that actually
    exist in this registry -- the workflow equivalent of
    check_dependency_resolvability, applied to `nodes[].capability_id`/
    `capability_version` instead of a capability's own `dependencies[]`."""
    workflows_dir = Path("workflows")
    capabilities_dir = Path("capabilities")
    if not workflows_dir.is_dir():
        return

    published_versions: dict = {}
    for contract_path in sorted(capabilities_dir.rglob("contract.json")):
        try:
            contract = json.loads(contract_path.read_text())
        except Exception:
            continue
        published_versions.setdefault(contract.get("id"), set()).add(contract.get("version"))

    for workflow_path in real_workflow_paths(workflows_dir):
        try:
            workflow = json.loads(workflow_path.read_text())
        except Exception:
            continue
        for node in workflow.get("nodes", []) or []:
            capability_id = node.get("capability_id")
            capability_version = node.get("capability_version")
            if not capability_id or not capability_version:
                continue
            if capability_version not in published_versions.get(capability_id, set()):
                fail(
                    errors,
                    "workflow.capability_reference_unresolvable",
                    str(workflow_path),
                    f"Node '{node.get('node_id')}' references {capability_id}@{capability_version}, "
                    "which is not a published capability in this registry",
                )


def check_immutability(base_sha: str, head_sha: str, errors: list) -> None:
    """FR: no PR may modify an existing contract.json/workflow.json once
    published (specs/001 FR-002/FR-013, specs/005 FR-002)."""
    for governed_dir, filename, error_code in (
        ("capabilities/", "contract.json", "capabilities.contract_modified"),
        ("workflows/", "workflow.json", "workflows.workflow_modified"),
    ):
        diff = subprocess.check_output(
            ["git", "diff", "--name-status", f"{base_sha}...{head_sha}", "--", governed_dir],
            text=True,
        )
        for line in diff.splitlines():
            if not line.strip():
                continue
            parts = line.split("\t")
            status = parts[0]
            path = parts[-1]
            if path.endswith(filename) and status != "A":
                fail(
                    errors,
                    error_code,
                    path,
                    f"{filename} must never be modified once published (git status: {status}). "
                    "Use a new version directory, or a deprecated.json sibling to yank.",
                )


def _semver_tuple(version: str):
    match = SEMVER_RE.match(version)
    if not match:
        return None
    return tuple(int(p) for p in version.split("+")[0].split("-")[0].split("."))


def classify_change(previous: dict, current: dict) -> str:
    """Returns 'major', 'minor', or 'patch' per specs/002-capability-validation.md."""
    prev_fields = set(previous.keys())
    curr_fields = set(current.keys())

    removed_required = [
        f for f in prev_fields - curr_fields if f in REQUIRED_FIELDS or f in previous.get("required", [])
    ]
    if removed_required:
        return "major"

    for field in prev_fields & curr_fields:
        if field in ("description",):
            continue
        if previous[field] != current[field]:
            if field in ("input_schema", "output_schema", "events", "permissions", "constraints"):
                return "major"

    if curr_fields - prev_fields:
        return "minor"

    return "patch"


def check_semver_bump(errors: list) -> None:
    """FR-002: declared bump must be >= detected change class vs. the prior version."""
    capabilities_dir = Path("capabilities")
    if not capabilities_dir.is_dir():
        return

    by_namespace_id: dict = {}
    for contract_path in sorted(capabilities_dir.rglob("contract.json")):
        parts = contract_path.parts
        try:
            idx = parts.index("capabilities")
            namespace_seg, id_seg, version_seg = parts[idx + 1], parts[idx + 2], parts[idx + 3]
        except (ValueError, IndexError):
            continue
        by_namespace_id.setdefault((namespace_seg, id_seg), []).append((version_seg, contract_path))

    for (namespace_seg, id_seg), versions in by_namespace_id.items():
        parsed = [(v, p, _semver_tuple(v)) for v, p in versions]
        parsed = [t for t in parsed if t[2] is not None]
        parsed.sort(key=lambda t: t[2])
        for i in range(1, len(parsed)):
            prev_version, prev_path, prev_tuple = parsed[i - 1]
            curr_version, curr_path, curr_tuple = parsed[i]
            try:
                previous = json.loads(prev_path.read_text())
                current = json.loads(curr_path.read_text())
            except Exception:
                continue
            change_class = classify_change(previous, current)
            bump = "major" if curr_tuple[0] > prev_tuple[0] else "minor" if curr_tuple[1] > prev_tuple[1] else "patch"
            rank = {"patch": 0, "minor": 1, "major": 2}
            if rank[bump] < rank[change_class]:
                fail(
                    errors,
                    "semver.bump_too_small",
                    str(curr_path),
                    f"Detected a '{change_class}' change from {prev_version} but version bump was only '{bump}'",
                )


def check_dependency_resolvability(errors: list) -> None:
    """FR-005: declared dependencies must resolve against already-published capabilities."""
    capabilities_dir = Path("capabilities")
    if not capabilities_dir.is_dir():
        return

    published: dict = {}
    for contract_path in sorted(capabilities_dir.rglob("contract.json")):
        try:
            contract = json.loads(contract_path.read_text())
        except Exception:
            continue
        key = (contract.get("namespace"), contract.get("id"))
        published.setdefault(key, []).append(contract.get("version"))

    for contract_path in sorted(capabilities_dir.rglob("contract.json")):
        try:
            contract = json.loads(contract_path.read_text())
        except Exception:
            continue
        for dep in contract.get("dependencies", []) or []:
            dep_id = dep.get("capability_id")
            dep_range = dep.get("version_range")
            if not dep_id:
                continue
            # capability_id may be "namespace/id" or just "id" (defaults to core namespace)
            if "/" in dep_id:
                dep_namespace, dep_short_id = dep_id.split("/", 1)
            else:
                dep_namespace, dep_short_id = "core", dep_id
            versions = published.get((dep_namespace, dep_short_id))
            if not versions:
                fail(
                    errors,
                    "dependency_unsatisfiable",
                    str(contract_path),
                    f"Dependency '{dep_id}' ({dep_range}) does not resolve to any published capability in this registry",
                )


def main() -> int:
    errors: list = []
    capabilities_dir = Path("capabilities")
    workflows_dir = Path("workflows")
    personas_dir = Path("personas")

    if capabilities_dir.is_dir():
        for contract_path in sorted(capabilities_dir.rglob("contract.json")):
            validate_contract(contract_path, errors)
        check_semver_bump(errors)
        check_dependency_resolvability(errors)

    if personas_dir.is_dir():
        for persona_path in sorted(personas_dir.rglob("persona.json")):
            validate_persona(persona_path, errors)
        check_persona_distinguished_from_resolves(errors)

    if workflows_dir.is_dir():
        for workflow_path in real_workflow_paths(workflows_dir):
            validate_workflow(workflow_path, errors)
        check_workflow_capability_references(errors)

    base_sha = None
    head_sha = None
    if len(sys.argv) >= 3:
        base_sha, head_sha = sys.argv[1], sys.argv[2]
    if base_sha and head_sha:
        try:
            check_immutability(base_sha, head_sha, errors)
        except subprocess.CalledProcessError as exc:
            fail(errors, "git.diff_failed", "capabilities/", f"Unable to compute diff: {exc}")
        try:
            check_new_scenarios_are_user_stories(base_sha, head_sha, errors)
        except subprocess.CalledProcessError as exc:
            fail(errors, "git.diff_failed", "capabilities/", f"Unable to compute diff: {exc}")
        try:
            check_new_use_cases_have_persona_ref(base_sha, head_sha, errors)
        except subprocess.CalledProcessError as exc:
            fail(errors, "git.diff_failed", "capabilities/", f"Unable to compute diff: {exc}")
        try:
            check_new_contracts_action_enum_coverage(base_sha, head_sha, errors)
        except subprocess.CalledProcessError as exc:
            fail(errors, "git.diff_failed", "capabilities/", f"Unable to compute diff: {exc}")
        try:
            check_new_contracts_have_artifact_reference(base_sha, head_sha, errors)
        except subprocess.CalledProcessError as exc:
            fail(errors, "git.diff_failed", "capabilities/", f"Unable to compute diff: {exc}")

    status = "passed" if not errors else "failed"
    print(json.dumps({"status": status, "failures": errors}, indent=2))

    if errors:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
