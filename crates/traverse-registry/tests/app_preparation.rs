//! Crate-external integration coverage for `specs/996-registry-app-preparation`.
//!
//! The in-module unit tests in `src/app_preparation.rs` exercise the pipeline
//! internals. This file proves the same contract *only through the public
//! re-exported surface* a Traverse host actually consumes -- no `crate::`
//! internals, no `pub(crate)` helpers -- so a regression that makes the API
//! unusable from outside the crate (FR-009: "consumable by Traverse without
//! CLI-specific reinterpretation of errors or evidence") fails here.

// Integration tests deliberately assert via `expect`/`panic!` on unreachable
// adapter paths; the crate's production lints forbid both.
#![allow(clippy::expect_used, clippy::panic)]

use std::fmt::Write as _;

use serde_json::json;
use sha2::{Digest, Sha256};
use traverse_contracts::{ExecutionTarget, Lifecycle, NetworkAccess};
use traverse_registry::{
    RegistryReference, ArtifactRetrievalAdapter, CacheCommitRejected, ContractRetrievalAdapter,
    HostPolicyDecision, RegistryPreparationFailureCode, RegistryPreparationRequest,
    RetrievalUnavailable, SelectedRecordRef, SignatureVerifier, SyncedPublicRegistryState,
    VerifiedCacheWriter, prepare_application_registry_reference,
};

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let mut out = String::new();
    for byte in hasher.finalize() {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn contract_bytes(ns: &str, id: &str, ver: &str, lifecycle: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
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

/// Build a synced-index snapshot the way a host obtains it -- by deserializing
/// a public `index.json`-shaped document, not by touching crate internals.
fn synced_state(
    ns: &str,
    id: &str,
    ver: &str,
    contract: &[u8],
    artifact: &[u8],
    deprecated: bool,
    permitted_targets: &[&str],
) -> SyncedPublicRegistryState {
    let doc = json!({
        "schema_version": "1.0.0",
        "workspace_id": "ws",
        "state_scope": "public_registry_synced",
        "source_repo": "traverse-framework/registry",
        "release_tag": "index-v1",
        "index_version": 1,
        "generated_at": "2026-09-08T00:00:00Z",
        "synced_at": "2026-09-08T00:00:00Z",
        "record_count": 1,
        "validation_status": "validated",
        "governing_spec": "055-registry-sync",
        "capabilities": [{
            "namespace": ns,
            "id": id,
            "version": ver,
            "digest": format!("sha256:{}", sha256_hex(artifact)),
            "artifact_url": "https://example.invalid/artifact.wasm",
            "contract_digest": format!("sha256:{}", sha256_hex(contract)),
            "contract_url": "https://example.invalid/contract.json",
            "deprecated": deprecated,
            "permitted_targets": permitted_targets,
            "lifecycle": "active"
        }],
        "events": []
    });
    serde_json::from_value(doc).expect("deserialize synced state")
}

struct StaticBytes(Vec<u8>);
impl ContractRetrievalAdapter for StaticBytes {
    fn fetch_contract(&self, _r: &SelectedRecordRef<'_>) -> Result<Vec<u8>, RetrievalUnavailable> {
        Ok(self.0.clone())
    }
}
impl ArtifactRetrievalAdapter for StaticBytes {
    fn fetch_artifact(&self, _r: &SelectedRecordRef<'_>) -> Result<Vec<u8>, RetrievalUnavailable> {
        Ok(self.0.clone())
    }
}

struct NeverCalled;
impl ContractRetrievalAdapter for NeverCalled {
    fn fetch_contract(&self, _r: &SelectedRecordRef<'_>) -> Result<Vec<u8>, RetrievalUnavailable> {
        panic!("FR-003: no retrieval adapter may be invoked once policy is denied");
    }
}
impl ArtifactRetrievalAdapter for NeverCalled {
    fn fetch_artifact(&self, _r: &SelectedRecordRef<'_>) -> Result<Vec<u8>, RetrievalUnavailable> {
        panic!("FR-003: no retrieval adapter may be invoked once policy is denied");
    }
}

struct FixedVerdict(bool);
impl SignatureVerifier for FixedVerdict {
    fn verify_artifact_signature(&self, _r: &SelectedRecordRef<'_>, _bytes: &[u8]) -> bool {
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

fn reference(range: &str) -> RegistryReference {
    RegistryReference {
        namespace: "core".to_string(),
        id: "core.calculate-price".to_string(),
        version_range: range.to_string(),
    }
}

#[test]
fn public_api_success_returns_serializable_secret_free_evidence() {
    let contract = contract_bytes("core", "core.calculate-price", "1.0.1", "active");
    let artifact = b"compiled wasm bytes".to_vec();
    let state = synced_state(
        "core",
        "core.calculate-price",
        "1.0.1",
        &contract,
        &artifact,
        false,
        &["cloud", "local"],
    );
    let mut cache = MemoryCache::default();

    let request = RegistryPreparationRequest {
        synced_state: &state,
        reference: reference("=1.0.1"),
        policy: HostPolicyDecision::Permitted,
        requested_target: ExecutionTarget::Cloud,
        requested_placement: "cloud".to_string(),
        supported_abi: "wasm/wasi-command".to_string(),
    };

    let prepared = prepare_application_registry_reference(
        &request,
        &StaticBytes(contract.clone()),
        &StaticBytes(artifact.clone()),
        &FixedVerdict(true),
        &mut cache,
    )
    .expect("preparation should succeed through the public API");

    assert_eq!(prepared.namespace, "core");
    assert_eq!(prepared.id, "core.calculate-price");
    assert_eq!(prepared.requested_range, "=1.0.1");
    assert_eq!(prepared.selected_version, "1.0.1");
    assert_eq!(prepared.trust_lifecycle, Lifecycle::Active);
    assert_eq!(prepared.abi, "wasm/wasi-command");
    assert_eq!(prepared.target, ExecutionTarget::Cloud);
    assert_eq!(prepared.placement, "cloud");
    assert_eq!(
        prepared.constraints.network_access,
        NetworkAccess::Forbidden
    );
    assert_eq!(
        prepared.contract_digest,
        state.capabilities[0].contract_digest
    );
    assert_eq!(prepared.artifact_digest, state.capabilities[0].digest);
    assert!(!prepared.resolver_evidence.is_empty());

    // FR-008: the public result must be serde-serializable (FR-009) and carry
    // no URL / endpoint / path / raw bytes even though the source index record
    // held `https://example.invalid/...` URLs.
    let wire = serde_json::to_string(&prepared).expect("evidence serializes");
    for forbidden in [
        "http",
        "example.invalid",
        "artifact_url",
        "contract_url",
        "://",
    ] {
        assert!(
            !wire.contains(forbidden),
            "serialized evidence leaked {forbidden:?}: {wire}"
        );
    }

    // Round-trips back through the public type (FR-009: no reinterpretation).
    let restored: traverse_registry::VerifiedRegistryPreparation =
        serde_json::from_str(&wire).expect("evidence round-trips");
    assert_eq!(restored, prepared);

    // Both digest-keyed cache entries were committed.
    assert_eq!(cache.entries.len(), 2);
}

#[test]
fn public_api_denied_policy_short_circuits_before_retrieval() {
    let contract = contract_bytes("core", "core.calculate-price", "1.0.1", "active");
    let artifact = b"bytes".to_vec();
    let state = synced_state(
        "core",
        "core.calculate-price",
        "1.0.1",
        &contract,
        &artifact,
        false,
        &["cloud"],
    );
    let mut cache = MemoryCache::default();

    let request = RegistryPreparationRequest {
        synced_state: &state,
        reference: reference("=1.0.1"),
        policy: HostPolicyDecision::Denied,
        requested_target: ExecutionTarget::Cloud,
        requested_placement: "cloud".to_string(),
        supported_abi: "wasm/wasi-command".to_string(),
    };

    let failure = prepare_application_registry_reference(
        &request,
        &NeverCalled,
        &NeverCalled,
        &FixedVerdict(true),
        &mut cache,
    )
    .expect_err("denied policy must fail");

    assert_eq!(
        failure.code,
        RegistryPreparationFailureCode::RegistryPolicyDenied
    );
    // Redacted `{code, message}` shape, serde-stable wire string (FR-005/009).
    let wire = serde_json::to_value(&failure).expect("failure serializes");
    assert_eq!(wire["code"], "registry_policy_denied");
    assert!(wire["message"].is_string());
    assert!(cache.entries.is_empty());
}

#[test]
fn public_api_exact_pin_has_no_fallback() {
    let contract = contract_bytes("core", "core.calculate-price", "1.0.0", "active");
    let artifact = b"bytes".to_vec();
    let state = synced_state(
        "core",
        "core.calculate-price",
        "1.0.0",
        &contract,
        &artifact,
        false,
        &["cloud"],
    );
    let mut cache = MemoryCache::default();

    let request = RegistryPreparationRequest {
        synced_state: &state,
        reference: reference("=1.0.1"),
        policy: HostPolicyDecision::Permitted,
        requested_target: ExecutionTarget::Cloud,
        requested_placement: "cloud".to_string(),
        supported_abi: "wasm/wasi-command".to_string(),
    };

    let failure = prepare_application_registry_reference(
        &request,
        &StaticBytes(contract),
        &StaticBytes(artifact),
        &FixedVerdict(true),
        &mut cache,
    )
    .expect_err("=1.0.1 must not resolve to 1.0.0");

    assert_eq!(
        failure.code,
        RegistryPreparationFailureCode::RegistryVersionRangeUnsatisfied
    );
    assert!(cache.entries.is_empty());
}

#[test]
fn public_api_cache_conflict_preserves_existing_entry() {
    let contract = contract_bytes("core", "core.calculate-price", "1.0.1", "active");
    let artifact = b"compiled wasm bytes".to_vec();
    let state = synced_state(
        "core",
        "core.calculate-price",
        "1.0.1",
        &contract,
        &artifact,
        false,
        &["cloud"],
    );
    let mut cache = MemoryCache::default();
    let artifact_key = state.capabilities[0].digest.clone();
    cache
        .commit(&artifact_key, b"previously stored different bytes")
        .expect("seed the cache");

    let request = RegistryPreparationRequest {
        synced_state: &state,
        reference: reference("=1.0.1"),
        policy: HostPolicyDecision::Permitted,
        requested_target: ExecutionTarget::Cloud,
        requested_placement: "cloud".to_string(),
        supported_abi: "wasm/wasi-command".to_string(),
    };

    let failure = prepare_application_registry_reference(
        &request,
        &StaticBytes(contract),
        &StaticBytes(artifact),
        &FixedVerdict(true),
        &mut cache,
    )
    .expect_err("conflicting bytes under an existing digest key");

    assert_eq!(
        failure.code,
        RegistryPreparationFailureCode::RegistryCacheCommitFailed
    );
    assert_eq!(
        cache.entries.get(&artifact_key).map(Vec::as_slice),
        Some(b"previously stored different bytes".as_slice()),
        "FR-007: the existing entry must be preserved unchanged"
    );
}

/// FR-005: the full first-failure taxonomy is public and every code has the
/// exact spec wire string, both via `as_str()` and via `serde`.
#[test]
fn public_api_exposes_the_whole_failure_taxonomy() {
    let all = [
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
    ];
    assert_eq!(all.len(), 12);
    for (code, wire) in all {
        assert_eq!(code.as_str(), wire);
        assert_eq!(
            serde_json::to_value(code).expect("serialize code"),
            json!(wire)
        );
        let back: RegistryPreparationFailureCode =
            serde_json::from_value(json!(wire)).expect("deserialize code");
        assert_eq!(back, code);
    }
}
