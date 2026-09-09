//! `specs/996-registry-app-preparation` -- the versioned Registry public
//! contract for host-run preparation of an application `registry_ref` from a
//! validated synced index.
//!
//! Traverse stays an offline consumer: this module performs no I/O and reads
//! no wall clock (`013-inherited-registry-governance`'s portability
//! constraint). The host supplies the synced-index snapshot, its own
//! network/host/permission policy verdict, the requested target / placement /
//! ABI, and the adapters it owns -- contract retrieval, artifact retrieval,
//! signature verification, and the verified cache writer. This module drives
//! them in one fixed order (FR-002 selection -> FR-003 policy -> FR-004
//! verification -> FR-007 cache commit) and returns either immutable,
//! non-secret evidence (FR-006) or the single first-failure code (FR-005).
//!
//! FR-008: no outcome or evidence value produced here carries a URL,
//! endpoint, credential, authorization header, host-private path, or raw
//! contract/artifact bytes -- the public types simply have no field for
//! them, and every `message` string is limited to public identity plus the
//! stage that failed.
//!
//! FR-009: every public type derives `serde` and the failure is a plain
//! `{ code, message }` with stable `snake_case` codes, so Traverse consumes
//! the result without CLI-specific reinterpretation.

use semver::{Op, Version, VersionReq};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use traverse_contracts::{ExecutionConstraints, ExecutionTarget, Lifecycle};

use std::path::Path;

use crate::application_manifest::{
    ApplicationBundleManifest, ApplicationManifestFailure, RegistryReference,
    WasmComponentManifest, unresolved_component_manifests,
};
use crate::public_registry_cache::{normalize_digest, sha256_hex};
use crate::public_registry_state::SyncedPublicRegistryState;

/// FR-005 first-failure taxonomy. Exactly these twelve codes; preparation
/// stops at the first one that applies, in the order the variants are
/// declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistryPreparationFailureCode {
    RegistryIndexSelectionFailed,
    RegistryVersionRangeUnsatisfied,
    RegistryLifecycleRejected,
    RegistryPolicyDenied,
    RegistryContractUnreachable,
    RegistryContractDigestMismatch,
    RegistryArtifactUnreachable,
    RegistryArtifactDigestMismatch,
    RegistrySignatureUnverified,
    RegistryAbiIncompatible,
    RegistryTargetIncompatible,
    RegistryCacheCommitFailed,
}

impl RegistryPreparationFailureCode {
    /// The stable wire string for this code -- identical to its `serde`
    /// representation, for callers that don't route through `serde`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RegistryIndexSelectionFailed => "registry_index_selection_failed",
            Self::RegistryVersionRangeUnsatisfied => "registry_version_range_unsatisfied",
            Self::RegistryLifecycleRejected => "registry_lifecycle_rejected",
            Self::RegistryPolicyDenied => "registry_policy_denied",
            Self::RegistryContractUnreachable => "registry_contract_unreachable",
            Self::RegistryContractDigestMismatch => "registry_contract_digest_mismatch",
            Self::RegistryArtifactUnreachable => "registry_artifact_unreachable",
            Self::RegistryArtifactDigestMismatch => "registry_artifact_digest_mismatch",
            Self::RegistrySignatureUnverified => "registry_signature_unverified",
            Self::RegistryAbiIncompatible => "registry_abi_incompatible",
            Self::RegistryTargetIncompatible => "registry_target_incompatible",
            Self::RegistryCacheCommitFailed => "registry_cache_commit_failed",
        }
    }
}

/// A stable, secret-free preparation failure (FR-005 / FR-008).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryPreparationFailure {
    pub code: RegistryPreparationFailureCode,
    /// Public identity plus the stage that failed. Never a URL, path,
    /// credential, or byte payload.
    pub message: String,
}

impl RegistryPreparationFailure {
    fn new(code: RegistryPreparationFailureCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

// The reference a host asks to prepare is `crate::RegistryReference`
// (`{ namespace, id, version_range }`, defined in `application_manifest`) --
// the same type an application manifest's `registry_ref` component carries,
// so a manifest-selected reference flows straight into a preparation request.
// `996` FR-002 requires `version_range` to be an exact `=X.Y.Z` range.

/// FR-003: the host evaluates its own network / host / permission policy and
/// hands preparation the verdict. Preparation invokes no retrieval adapter
/// when this is `Denied`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostPolicyDecision {
    Permitted,
    Denied,
}

/// FR-001 preparation request. Borrows the synced state so a large index is
/// never copied into this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryPreparationRequest<'a> {
    pub synced_state: &'a SyncedPublicRegistryState,
    pub reference: RegistryReference,
    pub policy: HostPolicyDecision,
    pub requested_target: ExecutionTarget,
    /// Free-form host placement label; recorded in evidence (FR-006). When it
    /// names an `ExecutionTarget` it is also checked against the record's
    /// permitted targets (FR-004); preparation never invents a fallback.
    pub requested_placement: String,
    /// The host-ABI identifier the caller can execute (for example
    /// `wasm/wasi-command`). Compared verbatim to the selected contract's
    /// declared ABI (FR-004).
    pub supported_abi: String,
}

/// The selected record's public identity, handed to a host adapter so it can
/// locate the bytes it owns. Deliberately carries no URL -- the host maps
/// identity to its own endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectedRecordRef<'a> {
    pub namespace: &'a str,
    pub id: &'a str,
    pub version: &'a str,
    pub contract_digest: &'a str,
    pub artifact_digest: &'a str,
}

/// A host retrieval adapter could not obtain the bytes it owns. Carries no
/// detail on purpose (FR-008) -- the reason is the host's to log privately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetrievalUnavailable;

/// A verified-cache commit was refused (I/O failure, or FR-007 conflict:
/// different bytes already stored under the same digest key).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheCommitRejected;

/// FR-001: host-owned contract retrieval.
pub trait ContractRetrievalAdapter {
    /// Return the canonical contract bytes for `record`.
    ///
    /// # Errors
    /// [`RetrievalUnavailable`] when the host cannot obtain them.
    fn fetch_contract(
        &self,
        record: &SelectedRecordRef<'_>,
    ) -> Result<Vec<u8>, RetrievalUnavailable>;
}

/// FR-001: host-owned artifact retrieval.
pub trait ArtifactRetrievalAdapter {
    /// Return the compiled artifact bytes for `record`.
    ///
    /// # Errors
    /// [`RetrievalUnavailable`] when the host cannot obtain them.
    fn fetch_artifact(
        &self,
        record: &SelectedRecordRef<'_>,
    ) -> Result<Vec<u8>, RetrievalUnavailable>;
}

/// FR-004: preparation drives signature verification but delegates the
/// Ed25519 curve check to the host, which owns the trust anchor and the
/// crypto -- the same delegation as retrieval I/O. Preparation still gates
/// on the result and maps a negative or errored verdict to
/// `registry_signature_unverified`.
pub trait SignatureVerifier {
    /// `true` iff the artifact's signature evidence is present, well-formed,
    /// and valid under a key the host trusts.
    fn verify_artifact_signature(
        &self,
        record: &SelectedRecordRef<'_>,
        artifact_bytes: &[u8],
    ) -> bool;
}

/// FR-007: host-owned verified cache writer.
pub trait VerifiedCacheWriter {
    /// Commit `bytes` under `digest_key`. If an entry already exists under
    /// `digest_key` and differs, the writer MUST reject the commit and keep
    /// the existing entry.
    ///
    /// # Errors
    /// [`CacheCommitRejected`] on any failure or conflict.
    fn commit(&mut self, digest_key: &str, bytes: &[u8]) -> Result<(), CacheCommitRejected>;
}

/// FR-006 immutable success evidence. FR-008: every field is public identity
/// or a declared classification -- no URL, endpoint, credential, header,
/// host path, or raw bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedRegistryPreparation {
    pub namespace: String,
    pub id: String,
    pub requested_range: String,
    pub selected_version: String,
    pub contract_digest: String,
    pub artifact_digest: String,
    pub trust_lifecycle: Lifecycle,
    pub abi: String,
    pub target: ExecutionTarget,
    pub placement: String,
    pub constraints: ExecutionConstraints,
    /// One line per ordered check that passed, in order -- a non-secret
    /// resolver trace.
    pub resolver_evidence: Vec<String>,
}

fn target_wire(target: &ExecutionTarget) -> &'static str {
    match target {
        ExecutionTarget::Local => "local",
        ExecutionTarget::Browser => "browser",
        ExecutionTarget::Edge => "edge",
        ExecutionTarget::Cloud => "cloud",
        ExecutionTarget::Worker => "worker",
        ExecutionTarget::Device => "device",
    }
}

fn lifecycle_from_str(value: &str) -> Option<Lifecycle> {
    match value {
        "draft" => Some(Lifecycle::Draft),
        "active" => Some(Lifecycle::Active),
        "deprecated" => Some(Lifecycle::Deprecated),
        "retired" => Some(Lifecycle::Retired),
        "archived" => Some(Lifecycle::Archived),
        _ => None,
    }
}

/// FR-002: a range that selects exactly one identical version -- `=X.Y.Z`
/// with all three components pinned.
fn exact_version(version_range: &str) -> Option<Version> {
    let requirement = VersionReq::parse(version_range).ok()?;
    let [comparator] = requirement.comparators.as_slice() else {
        return None;
    };
    if comparator.op != Op::Exact {
        return None;
    }
    let minor = comparator.minor?;
    let patch = comparator.patch?;
    Some(Version::new(comparator.major, minor, patch))
}

fn access_string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_string)
}

/// Pulls `execution.constraints` out of a parsed contract, defaulting each
/// dimension to its most permissive declared-absent value only when the
/// whole block is missing -- a present-but-partial block is a contract
/// authoring error the publish gate already rejects, so this is lenient by
/// design and never the thing that fails preparation.
fn constraints_from_contract(contract: &Value) -> ExecutionConstraints {
    use traverse_contracts::{FilesystemAccess, HostApiAccess, NetworkAccess};

    let block = contract.get("execution").and_then(|e| e.get("constraints"));
    let host_api = match access_string(block.and_then(|b| b.get("host_api_access"))).as_deref() {
        Some("exception_required") => HostApiAccess::ExceptionRequired,
        _ => HostApiAccess::None,
    };
    let network = match access_string(block.and_then(|b| b.get("network_access"))).as_deref() {
        Some("required") => NetworkAccess::Required,
        _ => NetworkAccess::Forbidden,
    };
    let filesystem = match access_string(block.and_then(|b| b.get("filesystem_access"))).as_deref()
    {
        Some("sandbox_only") => FilesystemAccess::SandboxOnly,
        _ => FilesystemAccess::None,
    };
    ExecutionConstraints {
        host_api_access: host_api,
        network_access: network,
        filesystem_access: filesystem,
    }
}

fn contract_abi(contract: &Value) -> String {
    let execution = contract.get("execution");
    let format = execution
        .and_then(|e| e.get("binary_format"))
        .and_then(Value::as_str)
        .unwrap_or("wasm");
    let entrypoint = execution
        .and_then(|e| e.get("entrypoint"))
        .and_then(|e| e.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or("wasi-command");
    format!("{format}/{entrypoint}")
}

/// FR-001..FR-009: prepare an application `registry_ref` from a validated
/// synced index.
///
/// Runs one fixed sequence -- exact-version selection, lifecycle, host
/// policy, contract retrieval + identity/digest verification, artifact
/// retrieval + digest verification, signature verification, ABI, target, and
/// verified cache commit -- stopping at the first failure with its stable
/// code (FR-005). On success it returns immutable, secret-free evidence
/// (FR-006 / FR-008). Policy is evaluated before any retrieval adapter is
/// touched (FR-003).
///
/// # Errors
/// [`RegistryPreparationFailure`] with the first-failure code.
#[allow(clippy::too_many_lines)]
pub fn prepare_application_registry_reference(
    request: &RegistryPreparationRequest<'_>,
    contract_adapter: &dyn ContractRetrievalAdapter,
    artifact_adapter: &dyn ArtifactRetrievalAdapter,
    signature_verifier: &dyn SignatureVerifier,
    cache: &mut dyn VerifiedCacheWriter,
) -> Result<VerifiedRegistryPreparation, RegistryPreparationFailure> {
    let reference = &request.reference;
    let identity = format!("{}:{}", reference.namespace, reference.id);
    let mut evidence: Vec<String> = Vec::new();

    // --- FR-002: exact selection, no fallback -----------------------------
    let Some(wanted) = exact_version(&reference.version_range) else {
        return Err(RegistryPreparationFailure::new(
            RegistryPreparationFailureCode::RegistryIndexSelectionFailed,
            format!(
                "{identity} version_range {} is not an exact =X.Y.Z range",
                reference.version_range
            ),
        ));
    };
    let wanted_string = wanted.to_string();

    let matches_identity = |ns: &str, id: &str| ns == reference.namespace && id == reference.id;
    let exact_record = request.synced_state.capabilities.iter().find(|record| {
        matches_identity(&record.namespace, &record.id) && record.version == wanted_string
    });
    let Some(record) = exact_record else {
        return Err(RegistryPreparationFailure::new(
            RegistryPreparationFailureCode::RegistryVersionRangeUnsatisfied,
            format!("{identity} has no active index record identical to {wanted_string}"),
        ));
    };
    if record.deprecated {
        return Err(RegistryPreparationFailure::new(
            RegistryPreparationFailureCode::RegistryLifecycleRejected,
            format!("{identity}@{wanted_string} is deprecated in the synced index"),
        ));
    }
    evidence.push(format!("index_selection: {wanted_string}"));

    // --- FR-003: host policy, before any retrieval adapter ---------------
    if request.policy == HostPolicyDecision::Denied {
        return Err(RegistryPreparationFailure::new(
            RegistryPreparationFailureCode::RegistryPolicyDenied,
            format!("host policy denied preparation of {identity}@{wanted_string}"),
        ));
    }
    evidence.push("policy: permitted".to_string());

    let selected = SelectedRecordRef {
        namespace: &record.namespace,
        id: &record.id,
        version: &record.version,
        contract_digest: &record.contract_digest,
        artifact_digest: &record.digest,
    };

    // --- FR-004: contract retrieval + identity + digest -----------------
    let contract_bytes = contract_adapter.fetch_contract(&selected).map_err(|_| {
        RegistryPreparationFailure::new(
            RegistryPreparationFailureCode::RegistryContractUnreachable,
            format!("host could not retrieve the contract for {identity}@{wanted_string}"),
        )
    })?;
    let contract_digest_mismatch = |detail: &str| {
        RegistryPreparationFailure::new(
            RegistryPreparationFailureCode::RegistryContractDigestMismatch,
            format!("{identity}@{wanted_string} contract {detail}"),
        )
    };
    match normalize_digest(&record.contract_digest) {
        Some(expected) if sha256_hex(&contract_bytes) == expected => {}
        _ => {
            return Err(contract_digest_mismatch(
                "bytes do not match the index digest",
            ));
        }
    }
    let contract: Value = serde_json::from_slice(&contract_bytes)
        .map_err(|_| contract_digest_mismatch("bytes are not valid JSON"))?;
    let identity_ok = contract.get("namespace").and_then(Value::as_str) == Some(&record.namespace)
        && contract.get("id").and_then(Value::as_str) == Some(&record.id)
        && contract.get("version").and_then(Value::as_str) == Some(&record.version);
    if !identity_ok {
        return Err(contract_digest_mismatch(
            "identity does not match the index record",
        ));
    }
    evidence.push("contract_digest: verified".to_string());

    // --- FR-004: lifecycle (contract is authoritative) -----------------
    let lifecycle_str = contract
        .get("lifecycle")
        .and_then(Value::as_str)
        .unwrap_or("active");
    let lifecycle = lifecycle_from_str(lifecycle_str).unwrap_or(Lifecycle::Draft);
    if lifecycle != Lifecycle::Active {
        return Err(RegistryPreparationFailure::new(
            RegistryPreparationFailureCode::RegistryLifecycleRejected,
            format!("{identity}@{wanted_string} contract lifecycle is {lifecycle_str}, not active"),
        ));
    }
    evidence.push("lifecycle: active".to_string());

    // --- FR-004: artifact retrieval + digest ---------------------------
    let artifact_bytes = artifact_adapter.fetch_artifact(&selected).map_err(|_| {
        RegistryPreparationFailure::new(
            RegistryPreparationFailureCode::RegistryArtifactUnreachable,
            format!("host could not retrieve the artifact for {identity}@{wanted_string}"),
        )
    })?;
    match normalize_digest(&record.digest) {
        Some(expected) if sha256_hex(&artifact_bytes) == expected => {}
        _ => {
            return Err(RegistryPreparationFailure::new(
                RegistryPreparationFailureCode::RegistryArtifactDigestMismatch,
                format!("{identity}@{wanted_string} artifact bytes do not match the index digest"),
            ));
        }
    }
    evidence.push("artifact_digest: verified".to_string());

    // --- FR-004: signature evidence ----------------------------------
    if !signature_verifier.verify_artifact_signature(&selected, &artifact_bytes) {
        return Err(RegistryPreparationFailure::new(
            RegistryPreparationFailureCode::RegistrySignatureUnverified,
            format!("{identity}@{wanted_string} artifact signature evidence did not verify"),
        ));
    }
    evidence.push("signature: verified".to_string());

    // --- FR-004: ABI ------------------------------------------------
    let abi = contract_abi(&contract);
    if abi != request.supported_abi {
        return Err(RegistryPreparationFailure::new(
            RegistryPreparationFailureCode::RegistryAbiIncompatible,
            format!(
                "{identity}@{wanted_string} declares ABI {abi}, host supports {}",
                request.supported_abi
            ),
        ));
    }
    evidence.push(format!("abi: {abi}"));

    // --- FR-004: permitted target / placement (no invented fallback) ---
    // An empty `permitted_targets` means "no restriction" -- preparation must
    // not invent a fallback, so it also must not reject on it.
    let permits = |wire: &str| {
        record.permitted_targets.is_empty()
            || record
                .permitted_targets
                .iter()
                .any(|permitted| permitted == wire)
    };
    let requested_target_wire = target_wire(&request.requested_target);
    if !permits(requested_target_wire) {
        return Err(RegistryPreparationFailure::new(
            RegistryPreparationFailureCode::RegistryTargetIncompatible,
            format!("{identity}@{wanted_string} does not permit target {requested_target_wire}"),
        ));
    }
    if let Some(placement_target) = placement_as_target(&request.requested_placement)
        && !permits(placement_target)
    {
        return Err(RegistryPreparationFailure::new(
            RegistryPreparationFailureCode::RegistryTargetIncompatible,
            format!("{identity}@{wanted_string} does not permit placement {placement_target}"),
        ));
    }
    evidence.push(format!("target: {requested_target_wire}"));

    // --- FR-007: verified cache commit ------------------------------
    let cache_reject = || {
        RegistryPreparationFailure::new(
            RegistryPreparationFailureCode::RegistryCacheCommitFailed,
            format!("verified cache rejected the commit for {identity}@{wanted_string}"),
        )
    };
    cache
        .commit(&record.contract_digest, &contract_bytes)
        .map_err(|_| cache_reject())?;
    cache
        .commit(&record.digest, &artifact_bytes)
        .map_err(|_| cache_reject())?;
    evidence.push("cache_commit: contract+artifact".to_string());

    // --- FR-006: immutable, secret-free success evidence -------------
    Ok(VerifiedRegistryPreparation {
        namespace: record.namespace.clone(),
        id: record.id.clone(),
        requested_range: reference.version_range.clone(),
        selected_version: record.version.clone(),
        contract_digest: record.contract_digest.clone(),
        artifact_digest: record.digest.clone(),
        trust_lifecycle: lifecycle,
        abi,
        target: request.requested_target.clone(),
        placement: request.requested_placement.clone(),
        constraints: constraints_from_contract(&contract),
        resolver_evidence: evidence,
    })
}

/// A placement label that happens to name an `ExecutionTarget` wire value,
/// so it can be checked against permitted targets too. A non-target label
/// (an arbitrary host zone name) returns `None` and is only recorded.
fn placement_as_target(placement: &str) -> Option<&'static str> {
    match placement {
        "local" => Some("local"),
        "browser" => Some("browser"),
        "edge" => Some("edge"),
        "cloud" => Some("cloud"),
        "worker" => Some("worker"),
        "device" => Some("device"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// specs/997-application-selected-preparation: prepare only the references an
// application manifest selected, and only those.
// ---------------------------------------------------------------------------

/// One manifest-declared registry reference plus the component's own
/// permitted-target narrowing (spec 997 FR-002 / FR-005). An empty
/// `permitted_targets` imposes no narrowing (no invented fallback).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedComponentReference {
    pub reference: RegistryReference,
    pub permitted_targets: Vec<ExecutionTarget>,
}

/// spec 997 FR-004 batch result: fail-fast. `failed` is `Some` iff the batch
/// is incomplete; `prepared` holds the evidence for every reference verified
/// before the stop, in selection order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchApplicationPreparation {
    pub prepared: Vec<VerifiedRegistryPreparation>,
    pub failed: Option<BatchPreparationFailure>,
}

/// The reference that stopped a batch, with its `996` first-failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchPreparationFailure {
    pub reference: RegistryReference,
    pub failure: RegistryPreparationFailure,
}

/// The one value each that applies to a whole batch (spec 997 FR-005): the
/// host's own policy verdict, the target it wants to run on, its placement
/// label, and the host-ABI it can execute. Per reference, `requested_target`
/// is additionally narrowed against that component's manifest
/// `permitted_targets`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchPreparationParams {
    pub policy: HostPolicyDecision,
    pub requested_target: ExecutionTarget,
    pub requested_placement: String,
    pub supported_abi: String,
}

/// spec 997 FR-001 / FR-002: the manifest-derived registry-reference set --
/// every `components[].manifest.registry_ref`, with that component's
/// `permitted_targets`, deduplicated by `(namespace, id, version_range)`
/// together with the target narrowing (differing narrowings are distinct
/// selections). This is the *only* reference input to
/// [`prepare_application_selected_references`]; a caller cannot supply a
/// reference that is not in it.
#[must_use]
pub fn application_selected_references(
    manifest: &ApplicationBundleManifest,
) -> Vec<SelectedComponentReference> {
    selected_component_references(
        manifest
            .components
            .iter()
            .map(|component| &component.manifest),
    )
}

/// spec 997 FR-002: the manifest-declared selection set, projected straight
/// from an application manifest's component-manifest tree on disk **without
/// resolving any `registry_ref` component**.
///
/// This is the unresolved-tree companion to [`application_selected_references`]:
/// that one needs an already-resolved [`ApplicationBundleManifest`], and hence
/// a `RegistryComponentResolver` backed by the prepared offline cache -- which
/// does not yet exist at the point a host must first choose *what* to prepare.
/// Both forms run the one crate-owned projection
/// ([`selected_component_references`]) over the same two per-component fields
/// (`registry_ref`, `permitted_targets`), so for any manifest that resolves
/// fully they return the identical set. Feed the result straight into
/// [`prepare_application_selected_references`].
///
/// Parsing and `registry_ref` shape validation are
/// [`unresolved_component_manifests`]'s; no contract, digest, dependency, or
/// resolver work happens.
///
/// # Errors
///
/// Returns [`ApplicationManifestFailure`] if the application manifest or any
/// referenced component manifest is missing, unreadable, unparseable,
/// declares duplicate component references, or declares an invalid component
/// source (`contract_path` / `registry_ref` exclusivity, or an incomplete
/// `registry_ref`).
pub fn application_selected_references_from_manifest_path(
    manifest_path: &Path,
) -> Result<Vec<SelectedComponentReference>, ApplicationManifestFailure> {
    let component_manifests = unresolved_component_manifests(manifest_path)?;
    Ok(selected_component_references(component_manifests.iter()))
}

/// The FR-002 extraction logic, over the component manifests directly.
/// [`application_selected_references`] is the public form over a whole
/// [`ApplicationBundleManifest`]; this is what its behaviour is unit-tested
/// against without a full manifest + contract graph.
pub(crate) fn selected_component_references<'a>(
    component_manifests: impl Iterator<Item = &'a WasmComponentManifest>,
) -> Vec<SelectedComponentReference> {
    let mut selected: Vec<SelectedComponentReference> = Vec::new();
    for manifest in component_manifests {
        let Some(reference) = manifest.registry_ref.clone() else {
            continue;
        };
        let candidate = SelectedComponentReference {
            reference,
            permitted_targets: manifest.permitted_targets.clone(),
        };
        if !selected.contains(&candidate) {
            selected.push(candidate);
        }
    }
    selected
}

/// spec 997 FR-001 / FR-003 / FR-004: prepare every reference an application
/// manifest selected -- and only those -- for offline activation.
///
/// This is the sanctioned entrypoint for activation preparation. It composes
/// [`prepare_application_registry_reference`] over `selected` in order,
/// stopping at the first reference that fails (fail-fast). The batch result
/// reports the evidence for each reference prepared before the stop and the
/// `(reference, failure)` pair for the one that failed. Cache entries
/// committed for earlier references stay valid (`996` FR-007: digest-keyed,
/// immutable), so a re-run resumes without re-fetching them.
///
/// `params` apply to the whole batch (FR-005); per reference, `requested_target`
/// is additionally verified against that component's manifest
/// `permitted_targets` (empty = no narrowing). An empty `selected` returns
/// `{ prepared: vec![], failed: None }` (FR-006).
#[must_use]
pub fn prepare_application_selected_references(
    selected: &[SelectedComponentReference],
    synced_state: &SyncedPublicRegistryState,
    params: &BatchPreparationParams,
    contract_adapter: &dyn ContractRetrievalAdapter,
    artifact_adapter: &dyn ArtifactRetrievalAdapter,
    signature_verifier: &dyn SignatureVerifier,
    cache: &mut dyn VerifiedCacheWriter,
) -> BatchApplicationPreparation {
    let mut prepared: Vec<VerifiedRegistryPreparation> = Vec::new();

    for component in selected {
        // spec 997 FR-005: the manifest may only tighten the index record's
        // permitted targets. An empty set imposes no narrowing.
        if !component.permitted_targets.is_empty()
            && !component
                .permitted_targets
                .contains(&params.requested_target)
        {
            return BatchApplicationPreparation {
                prepared,
                failed: Some(BatchPreparationFailure {
                    reference: component.reference.clone(),
                    failure: RegistryPreparationFailure {
                        code: RegistryPreparationFailureCode::RegistryTargetIncompatible,
                        message: format!(
                            "{}:{} manifest does not permit target {}",
                            component.reference.namespace,
                            component.reference.id,
                            target_wire(&params.requested_target)
                        ),
                    },
                }),
            };
        }

        let request = RegistryPreparationRequest {
            synced_state,
            reference: component.reference.clone(),
            policy: params.policy,
            requested_target: params.requested_target.clone(),
            requested_placement: params.requested_placement.clone(),
            supported_abi: params.supported_abi.clone(),
        };
        match prepare_application_registry_reference(
            &request,
            contract_adapter,
            artifact_adapter,
            signature_verifier,
            cache,
        ) {
            Ok(evidence) => prepared.push(evidence),
            Err(failure) => {
                return BatchApplicationPreparation {
                    prepared,
                    failed: Some(BatchPreparationFailure {
                        reference: component.reference.clone(),
                        failure,
                    }),
                };
            }
        }
    }

    BatchApplicationPreparation {
        prepared,
        failed: None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::public_registry_state::PublicRegistryCapabilityRecord;

    fn contract_json(ns: &str, id: &str, ver: &str, lifecycle: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "namespace": ns,
            "id": id,
            "version": ver,
            "lifecycle": lifecycle,
            "execution": {
                "binary_format": "wasm",
                "entrypoint": { "kind": "wasi-command" },
                "constraints": {
                    "host_api_access": "none",
                    "network_access": "forbidden",
                    "filesystem_access": "none"
                }
            }
        }))
        .expect("serialize contract")
    }

    fn record(
        ns: &str,
        id: &str,
        ver: &str,
        contract: &[u8],
        artifact: &[u8],
        deprecated: bool,
        targets: &[&str],
    ) -> PublicRegistryCapabilityRecord {
        PublicRegistryCapabilityRecord {
            namespace: ns.to_string(),
            id: id.to_string(),
            version: ver.to_string(),
            digest: format!("sha256:{}", sha256_hex(artifact)),
            artifact_url: String::new(),
            contract_digest: format!("sha256:{}", sha256_hex(contract)),
            contract_url: String::new(),
            deprecated,
            summary: String::new(),
            description: String::new(),
            use_cases: Vec::new(),
            service_type: String::new(),
            permitted_targets: targets.iter().map(|t| (*t).to_string()).collect(),
            lifecycle: "active".to_string(),
            provenance: None,
        }
    }

    fn synced(records: Vec<PublicRegistryCapabilityRecord>) -> SyncedPublicRegistryState {
        SyncedPublicRegistryState {
            schema_version: "1.0.0".to_string(),
            workspace_id: "ws".to_string(),
            state_scope: "public_registry_synced".to_string(),
            source_repo: "traverse-framework/registry".to_string(),
            release_tag: "index-v1".to_string(),
            index_version: 1,
            generated_at: "2026-09-08T00:00:00Z".to_string(),
            source_commit: None,
            synced_at: "2026-09-08T00:00:00Z".to_string(),
            record_count: records.len(),
            validation_status: "validated".to_string(),
            governing_spec: "055-registry-sync".to_string(),
            capabilities: records,
            events: Vec::new(),
        }
    }

    struct StaticContract(Vec<u8>);
    impl ContractRetrievalAdapter for StaticContract {
        fn fetch_contract(
            &self,
            _record: &SelectedRecordRef<'_>,
        ) -> Result<Vec<u8>, RetrievalUnavailable> {
            Ok(self.0.clone())
        }
    }
    struct StaticArtifact(Vec<u8>);
    impl ArtifactRetrievalAdapter for StaticArtifact {
        fn fetch_artifact(
            &self,
            _record: &SelectedRecordRef<'_>,
        ) -> Result<Vec<u8>, RetrievalUnavailable> {
            Ok(self.0.clone())
        }
    }
    struct Unreachable;
    impl ContractRetrievalAdapter for Unreachable {
        fn fetch_contract(
            &self,
            _record: &SelectedRecordRef<'_>,
        ) -> Result<Vec<u8>, RetrievalUnavailable> {
            Err(RetrievalUnavailable)
        }
    }
    impl ArtifactRetrievalAdapter for Unreachable {
        fn fetch_artifact(
            &self,
            _record: &SelectedRecordRef<'_>,
        ) -> Result<Vec<u8>, RetrievalUnavailable> {
            Err(RetrievalUnavailable)
        }
    }
    struct Signature(bool);
    impl SignatureVerifier for Signature {
        fn verify_artifact_signature(
            &self,
            _record: &SelectedRecordRef<'_>,
            _artifact_bytes: &[u8],
        ) -> bool {
            self.0
        }
    }
    #[derive(Default)]
    struct MemoryCache {
        entries: std::collections::BTreeMap<String, Vec<u8>>,
    }
    impl VerifiedCacheWriter for MemoryCache {
        fn commit(&mut self, digest_key: &str, bytes: &[u8]) -> Result<(), CacheCommitRejected> {
            if let Some(existing) = self.entries.get(digest_key) {
                if existing != bytes {
                    return Err(CacheCommitRejected);
                }
                return Ok(());
            }
            self.entries.insert(digest_key.to_string(), bytes.to_vec());
            Ok(())
        }
    }
    struct AlwaysRejectCache;
    impl VerifiedCacheWriter for AlwaysRejectCache {
        fn commit(&mut self, _digest_key: &str, _bytes: &[u8]) -> Result<(), CacheCommitRejected> {
            Err(CacheCommitRejected)
        }
    }

    fn request<'a>(
        state: &'a SyncedPublicRegistryState,
        version_range: &str,
        policy: HostPolicyDecision,
    ) -> RegistryPreparationRequest<'a> {
        RegistryPreparationRequest {
            synced_state: state,
            reference: RegistryReference {
                namespace: "core".to_string(),
                id: "core.calculate-price".to_string(),
                version_range: version_range.to_string(),
            },
            policy,
            requested_target: ExecutionTarget::Cloud,
            requested_placement: "cloud".to_string(),
            supported_abi: "wasm/wasi-command".to_string(),
        }
    }

    fn happy_state() -> (SyncedPublicRegistryState, Vec<u8>, Vec<u8>) {
        let contract = contract_json("core", "core.calculate-price", "1.0.1", "active");
        let artifact = b"compiled wasm bytes".to_vec();
        let state = synced(vec![record(
            "core",
            "core.calculate-price",
            "1.0.1",
            &contract,
            &artifact,
            false,
            &["cloud", "local"],
        )]);
        (state, contract, artifact)
    }

    #[test]
    fn scenario_1_exact_active_returns_immutable_evidence() {
        let (state, contract, artifact) = happy_state();
        let mut cache = MemoryCache::default();
        let ok = prepare_application_registry_reference(
            &request(&state, "=1.0.1", HostPolicyDecision::Permitted),
            &StaticContract(contract.clone()),
            &StaticArtifact(artifact.clone()),
            &Signature(true),
            &mut cache,
        )
        .expect("preparation should succeed");

        assert_eq!(ok.selected_version, "1.0.1");
        assert_eq!(ok.requested_range, "=1.0.1");
        assert_eq!(ok.trust_lifecycle, Lifecycle::Active);
        assert_eq!(ok.abi, "wasm/wasi-command");
        assert_eq!(ok.target, ExecutionTarget::Cloud);
        assert_eq!(
            ok.constraints.network_access,
            traverse_contracts::NetworkAccess::Forbidden
        );
        assert_eq!(
            ok.resolver_evidence,
            vec![
                "index_selection: 1.0.1",
                "policy: permitted",
                "contract_digest: verified",
                "lifecycle: active",
                "artifact_digest: verified",
                "signature: verified",
                "abi: wasm/wasi-command",
                "target: cloud",
                "cache_commit: contract+artifact",
            ]
        );
        // FR-008: evidence must not leak the (empty here, but still) URLs.
        let json = serde_json::to_string(&ok).expect("serialize evidence");
        assert!(!json.contains("artifact_url") && !json.contains("http"));
    }

    #[test]
    fn scenario_2_only_lower_version_is_unsatisfied_with_no_fallback() {
        let contract = contract_json("core", "core.calculate-price", "1.0.0", "active");
        let artifact = b"bytes".to_vec();
        let state = synced(vec![record(
            "core",
            "core.calculate-price",
            "1.0.0",
            &contract,
            &artifact,
            false,
            &["cloud"],
        )]);
        let mut cache = MemoryCache::default();
        let err = prepare_application_registry_reference(
            &request(&state, "=1.0.1", HostPolicyDecision::Permitted),
            &StaticContract(contract),
            &StaticArtifact(artifact),
            &Signature(true),
            &mut cache,
        )
        .expect_err("no identical active version");
        assert_eq!(
            err.code,
            RegistryPreparationFailureCode::RegistryVersionRangeUnsatisfied
        );
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn non_exact_range_is_index_selection_failed() {
        let (state, contract, artifact) = happy_state();
        let mut cache = MemoryCache::default();
        let err = prepare_application_registry_reference(
            &request(&state, "^1.0.0", HostPolicyDecision::Permitted),
            &StaticContract(contract),
            &StaticArtifact(artifact),
            &Signature(true),
            &mut cache,
        )
        .expect_err("caret range is not exact");
        assert_eq!(
            err.code,
            RegistryPreparationFailureCode::RegistryIndexSelectionFailed
        );
    }

    #[test]
    fn deprecated_exact_version_is_lifecycle_rejected() {
        let contract = contract_json("core", "core.calculate-price", "1.0.1", "active");
        let artifact = b"bytes".to_vec();
        let state = synced(vec![record(
            "core",
            "core.calculate-price",
            "1.0.1",
            &contract,
            &artifact,
            true,
            &["cloud"],
        )]);
        let mut cache = MemoryCache::default();
        let err = prepare_application_registry_reference(
            &request(&state, "=1.0.1", HostPolicyDecision::Permitted),
            &StaticContract(contract),
            &StaticArtifact(artifact),
            &Signature(true),
            &mut cache,
        )
        .expect_err("deprecated record");
        assert_eq!(
            err.code,
            RegistryPreparationFailureCode::RegistryLifecycleRejected
        );
    }

    #[test]
    fn scenario_3_denied_policy_returns_before_any_retrieval() {
        let (state, _c, _a) = happy_state();
        let mut cache = MemoryCache::default();
        let err = prepare_application_registry_reference(
            &request(&state, "=1.0.1", HostPolicyDecision::Denied),
            &Unreachable,
            &Unreachable,
            &Signature(true),
            &mut cache,
        )
        .expect_err("policy denied");
        assert_eq!(
            err.code,
            RegistryPreparationFailureCode::RegistryPolicyDenied
        );
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn contract_unreachable_maps_to_its_code() {
        let (state, _c, artifact) = happy_state();
        let mut cache = MemoryCache::default();
        let err = prepare_application_registry_reference(
            &request(&state, "=1.0.1", HostPolicyDecision::Permitted),
            &Unreachable,
            &StaticArtifact(artifact),
            &Signature(true),
            &mut cache,
        )
        .expect_err("contract fetch fails");
        assert_eq!(
            err.code,
            RegistryPreparationFailureCode::RegistryContractUnreachable
        );
    }

    #[test]
    fn scenario_4_contract_digest_mismatch() {
        let (state, _c, artifact) = happy_state();
        let mut cache = MemoryCache::default();
        let err = prepare_application_registry_reference(
            &request(&state, "=1.0.1", HostPolicyDecision::Permitted),
            &StaticContract(b"not the contract".to_vec()),
            &StaticArtifact(artifact),
            &Signature(true),
            &mut cache,
        )
        .expect_err("wrong contract bytes");
        assert_eq!(
            err.code,
            RegistryPreparationFailureCode::RegistryContractDigestMismatch
        );
    }

    #[test]
    fn scenario_4_artifact_digest_mismatch_writes_no_cache() {
        let (state, contract, _a) = happy_state();
        let mut cache = MemoryCache::default();
        let err = prepare_application_registry_reference(
            &request(&state, "=1.0.1", HostPolicyDecision::Permitted),
            &StaticContract(contract),
            &StaticArtifact(b"tampered".to_vec()),
            &Signature(true),
            &mut cache,
        )
        .expect_err("wrong artifact bytes");
        assert_eq!(
            err.code,
            RegistryPreparationFailureCode::RegistryArtifactDigestMismatch
        );
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn scenario_4_unverified_signature() {
        let (state, contract, artifact) = happy_state();
        let mut cache = MemoryCache::default();
        let err = prepare_application_registry_reference(
            &request(&state, "=1.0.1", HostPolicyDecision::Permitted),
            &StaticContract(contract),
            &StaticArtifact(artifact),
            &Signature(false),
            &mut cache,
        )
        .expect_err("signature does not verify");
        assert_eq!(
            err.code,
            RegistryPreparationFailureCode::RegistrySignatureUnverified
        );
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn scenario_4_abi_incompatible() {
        let (state, contract, artifact) = happy_state();
        let mut req = request(&state, "=1.0.1", HostPolicyDecision::Permitted);
        req.supported_abi = "wasm/component".to_string();
        let mut cache = MemoryCache::default();
        let err = prepare_application_registry_reference(
            &req,
            &StaticContract(contract),
            &StaticArtifact(artifact),
            &Signature(true),
            &mut cache,
        )
        .expect_err("abi differs");
        assert_eq!(
            err.code,
            RegistryPreparationFailureCode::RegistryAbiIncompatible
        );
    }

    #[test]
    fn scenario_4_target_incompatible() {
        let contract = contract_json("core", "core.calculate-price", "1.0.1", "active");
        let artifact = b"bytes".to_vec();
        let state = synced(vec![record(
            "core",
            "core.calculate-price",
            "1.0.1",
            &contract,
            &artifact,
            false,
            &["local", "edge"],
        )]);
        let mut cache = MemoryCache::default();
        let err = prepare_application_registry_reference(
            &request(&state, "=1.0.1", HostPolicyDecision::Permitted),
            &StaticContract(contract),
            &StaticArtifact(artifact),
            &Signature(true),
            &mut cache,
        )
        .expect_err("cloud target not permitted");
        assert_eq!(
            err.code,
            RegistryPreparationFailureCode::RegistryTargetIncompatible
        );
    }

    #[test]
    fn empty_permitted_targets_do_not_invent_a_fallback_failure() {
        let contract = contract_json("core", "core.calculate-price", "1.0.1", "active");
        let artifact = b"bytes".to_vec();
        let state = synced(vec![record(
            "core",
            "core.calculate-price",
            "1.0.1",
            &contract,
            &artifact,
            false,
            &[],
        )]);
        let mut cache = MemoryCache::default();
        let ok = prepare_application_registry_reference(
            &request(&state, "=1.0.1", HostPolicyDecision::Permitted),
            &StaticContract(contract),
            &StaticArtifact(artifact),
            &Signature(true),
            &mut cache,
        )
        .expect("empty permitted_targets means no target restriction");
        assert_eq!(ok.target, ExecutionTarget::Cloud);
    }

    #[test]
    fn scenario_5_cache_conflict_is_commit_failed_and_preserves_existing() {
        let (state, contract, artifact) = happy_state();
        let mut cache = MemoryCache::default();
        let record_digest = state.capabilities[0].digest.clone();
        cache
            .commit(&record_digest, b"original different bytes")
            .expect("seed the cache");
        let err = prepare_application_registry_reference(
            &request(&state, "=1.0.1", HostPolicyDecision::Permitted),
            &StaticContract(contract),
            &StaticArtifact(artifact),
            &Signature(true),
            &mut cache,
        )
        .expect_err("conflicting bytes under the artifact digest key");
        assert_eq!(
            err.code,
            RegistryPreparationFailureCode::RegistryCacheCommitFailed
        );
        assert_eq!(
            cache.entries.get(&record_digest).map(Vec::as_slice),
            Some(b"original different bytes".as_slice())
        );
    }

    #[test]
    fn cache_writer_failure_is_commit_failed() {
        let (state, contract, artifact) = happy_state();
        let err = prepare_application_registry_reference(
            &request(&state, "=1.0.1", HostPolicyDecision::Permitted),
            &StaticContract(contract),
            &StaticArtifact(artifact),
            &Signature(true),
            &mut AlwaysRejectCache,
        )
        .expect_err("cache writer rejects");
        assert_eq!(
            err.code,
            RegistryPreparationFailureCode::RegistryCacheCommitFailed
        );
    }

    #[test]
    fn failure_code_wire_strings_match_fr_005() {
        for (code, wire) in [
            (
                RegistryPreparationFailureCode::RegistryIndexSelectionFailed,
                "registry_index_selection_failed",
            ),
            (
                RegistryPreparationFailureCode::RegistryVersionRangeUnsatisfied,
                "registry_version_range_unsatisfied",
            ),
            (
                RegistryPreparationFailureCode::RegistryLifecycleRejected,
                "registry_lifecycle_rejected",
            ),
            (
                RegistryPreparationFailureCode::RegistryPolicyDenied,
                "registry_policy_denied",
            ),
            (
                RegistryPreparationFailureCode::RegistryContractUnreachable,
                "registry_contract_unreachable",
            ),
            (
                RegistryPreparationFailureCode::RegistryContractDigestMismatch,
                "registry_contract_digest_mismatch",
            ),
            (
                RegistryPreparationFailureCode::RegistryArtifactUnreachable,
                "registry_artifact_unreachable",
            ),
            (
                RegistryPreparationFailureCode::RegistryArtifactDigestMismatch,
                "registry_artifact_digest_mismatch",
            ),
            (
                RegistryPreparationFailureCode::RegistrySignatureUnverified,
                "registry_signature_unverified",
            ),
            (
                RegistryPreparationFailureCode::RegistryAbiIncompatible,
                "registry_abi_incompatible",
            ),
            (
                RegistryPreparationFailureCode::RegistryTargetIncompatible,
                "registry_target_incompatible",
            ),
            (
                RegistryPreparationFailureCode::RegistryCacheCommitFailed,
                "registry_cache_commit_failed",
            ),
        ] {
            assert_eq!(code.as_str(), wire);
            assert_eq!(
                serde_json::to_value(code).expect("serialize code"),
                serde_json::Value::String(wire.to_string())
            );
        }
    }

    // --- specs/997-application-selected-preparation -----------------------

    use crate::application_manifest::{ComponentExecutionMode, WasmComponentManifest};

    /// `(capability id, contract bytes, artifact bytes)` for `ByIdAdapter`.
    type ByIdEntry = (&'static str, Vec<u8>, Vec<u8>);

    fn wasm_manifest(
        registry_ref: Option<(&str, &str, &str)>,
        targets: &[ExecutionTarget],
    ) -> WasmComponentManifest {
        WasmComponentManifest {
            component_id: "c".to_string(),
            version: "1.0.0".to_string(),
            schema_version: "1.0.0".to_string(),
            execution_mode: ComponentExecutionMode::Wasm,
            capability_id: registry_ref
                .map(|(_, id, _)| id.to_string())
                .unwrap_or_default(),
            capability_version: registry_ref
                .map(|(_, _, v)| v.to_string())
                .unwrap_or_default(),
            contract_path: None,
            registry_ref: registry_ref.map(|(ns, id, ver)| RegistryReference {
                namespace: ns.to_string(),
                id: id.to_string(),
                version_range: format!("={ver}"),
            }),
            wasm_binary_path: None,
            wasm_digest: None,
            platforms: Vec::new(),
            wrapper_path: None,
            runtime_constraints: Value::Null,
            permitted_targets: targets.to_vec(),
            dependencies: Vec::new(),
            connector_requirements: Vec::new(),
            validation_evidence: Vec::new(),
            executable_pin: None,
        }
    }

    fn params(target: ExecutionTarget, policy: HostPolicyDecision) -> BatchPreparationParams {
        BatchPreparationParams {
            policy,
            requested_target: target,
            requested_placement: "cloud".to_string(),
            supported_abi: "wasm/wasi-command".to_string(),
        }
    }

    /// A retrieval adapter that maps a capability id to its bytes, so a batch
    /// test can give each selected reference distinct contract/artifact
    /// content. Artifact bytes are behind a `RefCell` so a test can fix a
    /// retrieval mid-way and re-run.
    struct ByIdAdapter {
        contracts: std::collections::BTreeMap<String, Vec<u8>>,
        artifacts: std::cell::RefCell<std::collections::BTreeMap<String, Vec<u8>>>,
    }
    impl ByIdAdapter {
        fn new(entries: &[ByIdEntry]) -> Self {
            let mut contracts = std::collections::BTreeMap::new();
            let mut artifacts = std::collections::BTreeMap::new();
            for (id, contract, artifact) in entries {
                contracts.insert((*id).to_string(), contract.clone());
                artifacts.insert((*id).to_string(), artifact.clone());
            }
            Self {
                contracts,
                artifacts: std::cell::RefCell::new(artifacts),
            }
        }
        fn set_artifact(&self, id: &str, bytes: Vec<u8>) {
            self.artifacts.borrow_mut().insert(id.to_string(), bytes);
        }
    }
    impl ContractRetrievalAdapter for ByIdAdapter {
        fn fetch_contract(
            &self,
            r: &SelectedRecordRef<'_>,
        ) -> Result<Vec<u8>, RetrievalUnavailable> {
            self.contracts
                .get(r.id)
                .cloned()
                .ok_or(RetrievalUnavailable)
        }
    }
    impl ArtifactRetrievalAdapter for ByIdAdapter {
        fn fetch_artifact(
            &self,
            r: &SelectedRecordRef<'_>,
        ) -> Result<Vec<u8>, RetrievalUnavailable> {
            self.artifacts
                .borrow()
                .get(r.id)
                .cloned()
                .ok_or(RetrievalUnavailable)
        }
    }

    /// SC-002: extraction pulls every declared reference, deduplicates
    /// `(namespace, id, version_range)` + narrowing, and skips local
    /// components.
    #[test]
    fn selected_references_are_the_declared_distinct_set() {
        let manifests = [
            wasm_manifest(Some(("core", "core.a", "1.0.0")), &[ExecutionTarget::Cloud]),
            wasm_manifest(Some(("core", "core.b", "2.0.0")), &[]),
            // exact duplicate of the first -> collapsed
            wasm_manifest(Some(("core", "core.a", "1.0.0")), &[ExecutionTarget::Cloud]),
            // same ref, different narrowing -> distinct selection
            wasm_manifest(Some(("core", "core.a", "1.0.0")), &[ExecutionTarget::Local]),
            // local component (no registry_ref) -> skipped
            wasm_manifest(None, &[]),
        ];
        let selected = selected_component_references(manifests.iter());
        assert_eq!(selected.len(), 3);
        assert_eq!(selected[0].reference.id, "core.a");
        assert_eq!(selected[0].permitted_targets, vec![ExecutionTarget::Cloud]);
        assert_eq!(selected[1].reference.id, "core.b");
        assert_eq!(selected[2].reference.id, "core.a");
        assert_eq!(selected[2].permitted_targets, vec![ExecutionTarget::Local]);
    }

    fn selected(
        ns: &str,
        id: &str,
        ver: &str,
        targets: &[ExecutionTarget],
    ) -> SelectedComponentReference {
        SelectedComponentReference {
            reference: RegistryReference {
                namespace: ns.to_string(),
                id: id.to_string(),
                version_range: format!("={ver}"),
            },
            permitted_targets: targets.to_vec(),
        }
    }

    /// SC-005: an application with only local components prepares trivially.
    #[test]
    fn empty_selection_prepares_trivially() {
        let (state, contract, artifact) = happy_state();
        let mut cache = MemoryCache::default();
        let out = prepare_application_selected_references(
            &[],
            &state,
            &params(ExecutionTarget::Cloud, HostPolicyDecision::Permitted),
            &StaticContract(contract),
            &StaticArtifact(artifact),
            &Signature(true),
            &mut cache,
        );
        assert!(out.prepared.is_empty());
        assert!(out.failed.is_none());
        assert!(cache.entries.is_empty());
    }

    fn two_capability_state() -> (SyncedPublicRegistryState, Vec<ByIdEntry>) {
        let c1 = contract_json("core", "core.one", "1.0.1", "active");
        let a1 = b"artifact one".to_vec();
        let c2 = contract_json("core", "core.two", "2.0.0", "active");
        let a2 = b"artifact two".to_vec();
        let state = synced(vec![
            record("core", "core.one", "1.0.1", &c1, &a1, false, &["cloud"]),
            record("core", "core.two", "2.0.0", &c2, &a2, false, &["cloud"]),
        ]);
        (state, vec![("core.one", c1, a1), ("core.two", c2, a2)])
    }

    fn two_selected() -> [SelectedComponentReference; 2] {
        [
            selected("core", "core.one", "1.0.1", &[ExecutionTarget::Cloud]),
            selected("core", "core.two", "2.0.0", &[ExecutionTarget::Cloud]),
        ]
    }

    /// Happy path: every selected reference is prepared, in order.
    #[test]
    fn batch_prepares_every_selected_reference() {
        let (state, entries) = two_capability_state();
        let adapters = ByIdAdapter::new(&entries);
        let mut cache = MemoryCache::default();
        let out = prepare_application_selected_references(
            &two_selected(),
            &state,
            &params(ExecutionTarget::Cloud, HostPolicyDecision::Permitted),
            &adapters,
            &adapters,
            &Signature(true),
            &mut cache,
        );
        assert!(out.failed.is_none());
        assert_eq!(out.prepared.len(), 2);
        assert_eq!(out.prepared[0].id, "core.one");
        assert_eq!(out.prepared[1].id, "core.two");
    }

    /// SC-003: fail-fast keeps the refs prepared before the stop, reports the
    /// failing ref + its 996 code, and a re-run resumes over the valid cache.
    #[test]
    fn batch_is_fail_fast_and_resumable() {
        let (state, mut entries) = two_capability_state();
        // Break core.two's artifact so its digest won't match.
        entries[1].2 = b"WRONG bytes".to_vec();
        let adapters = ByIdAdapter::new(&entries);
        let refs = two_selected();
        let mut cache = MemoryCache::default();

        let first = prepare_application_selected_references(
            &refs,
            &state,
            &params(ExecutionTarget::Cloud, HostPolicyDecision::Permitted),
            &adapters,
            &adapters,
            &Signature(true),
            &mut cache,
        );
        assert_eq!(first.prepared.len(), 1);
        assert_eq!(first.prepared[0].id, "core.one");
        let failure = first.failed.expect("second ref must fail");
        assert_eq!(failure.reference.id, "core.two");
        assert_eq!(
            failure.failure.code,
            RegistryPreparationFailureCode::RegistryArtifactDigestMismatch
        );
        // core.one's cache entries survived the batch failure; core.two's did not.
        assert!(cache.entries.contains_key(&state.capabilities[0].digest));
        assert!(!cache.entries.contains_key(&state.capabilities[1].digest));

        // Fix core.two's retrieval and re-run: core.one is a cache hit, batch completes.
        adapters.set_artifact("core.two", b"artifact two".to_vec());
        let second = prepare_application_selected_references(
            &refs,
            &state,
            &params(ExecutionTarget::Cloud, HostPolicyDecision::Permitted),
            &adapters,
            &adapters,
            &Signature(true),
            &mut cache,
        );
        assert!(second.failed.is_none());
        assert_eq!(second.prepared.len(), 2);
    }

    /// SC-004: a target the manifest component excludes fails with 996's
    /// `registry_target_incompatible`, before that ref's pipeline runs.
    #[test]
    fn manifest_target_narrowing_rejects_before_the_pipeline() {
        let (state, contract, artifact) = happy_state();
        let mut cache = MemoryCache::default();
        let out = prepare_application_selected_references(
            // index record permits cloud+local, but the manifest narrows to local
            &[selected(
                "core",
                "core.calculate-price",
                "1.0.1",
                &[ExecutionTarget::Local],
            )],
            &state,
            &params(ExecutionTarget::Cloud, HostPolicyDecision::Permitted),
            &StaticContract(contract),
            &StaticArtifact(artifact),
            &Signature(true),
            &mut cache,
        );
        let failure = out.failed.expect("manifest narrowing rejects cloud");
        assert_eq!(
            failure.failure.code,
            RegistryPreparationFailureCode::RegistryTargetIncompatible
        );
        assert!(out.prepared.is_empty());
        assert!(cache.entries.is_empty());
    }

    /// SC-001: the only public producer of the batch's reference input is
    /// `application_selected_references` / `selected_component_references` --
    /// there is no path from a caller-built string to a prepared reference
    /// that doesn't pass through the manifest extraction.
    #[test]
    fn selection_set_only_comes_from_the_manifest_extractor() {
        let manifests = [wasm_manifest(Some(("core", "core.x", "1.2.3")), &[])];
        let selected = selected_component_references(manifests.iter());
        assert_eq!(selected[0].reference.version_range, "=1.2.3");
        // `SelectedComponentReference` has public fields, but the batch entry
        // point takes `&[SelectedComponentReference]` and this fn is the
        // sanctioned way to build that slice from an application manifest.
    }
}
