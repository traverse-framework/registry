#![allow(clippy::expect_used)]

use serde_json::json;
use traverse_contracts::{ExecutionTarget, reference_connector_contracts};
use traverse_registry::{
    ApplicationConnectorBinding, CapabilityRegistry, ConnectorActivationDrift,
    ConnectorActivationErrorCode, ConnectorActivationRequest, ConnectorRegistration,
    InstalledConnector, LookupScope, RegistryScope, detect_activation_drift,
    validate_connector_activation,
};

fn http_connector_contract(version: &str) -> traverse_contracts::ConnectorContract {
    let mut contract = reference_connector_contracts()
        .into_iter()
        .find(|contract| contract.connector_id == "traverse.http")
        .expect("traverse.http reference connector should exist");
    contract.version = version.to_string();
    contract
}

fn register_http_connector(registry: &mut CapabilityRegistry, version: &str) {
    registry
        .register_connector(ConnectorRegistration {
            scope: RegistryScope::Public,
            contract_path: format!("registry/public/connectors/traverse.http/{version}.json"),
            registered_at: "2026-08-11T00:00:00Z".to_string(),
            governing_spec: "039-connector-plugin-architecture".to_string(),
            validator_version: "registry-test".to_string(),
            contract: http_connector_contract(version),
        })
        .expect("connector registration should pass");
}

fn http_binding(version_range: &str) -> ApplicationConnectorBinding {
    ApplicationConnectorBinding {
        connector_id: "traverse.http".to_string(),
        version_range: version_range.to_string(),
        config_ref: "http.default".to_string(),
    }
}

fn activation_request(
    installed_version: &str,
    host_config: serde_json::Value,
) -> ConnectorActivationRequest {
    ConnectorActivationRequest {
        connector_id: "traverse.http".to_string(),
        installed: InstalledConnector {
            connector_id: "traverse.http".to_string(),
            version: installed_version.to_string(),
        },
        placement_target: ExecutionTarget::Local,
        host_config,
    }
}

#[test]
fn activates_valid_binding_and_produces_non_secret_evidence() {
    let mut registry = CapabilityRegistry::new();
    register_http_connector(&mut registry, "1.0.0");
    let bindings = vec![http_binding("^1.0.0")];
    let request = activation_request("1.0.0", json!({ "base_url": "https://example.com" }));

    let evidence =
        validate_connector_activation(&registry, LookupScope::PublicOnly, &bindings, &request)
            .expect("a satisfied binding with valid config should activate");

    assert_eq!(evidence.connector_id, "traverse.http");
    assert_eq!(evidence.resolved_version, "1.0.0");
    assert_eq!(evidence.placement_target, ExecutionTarget::Local);
    assert_eq!(evidence.config_keys_present, vec!["base_url".to_string()]);
    assert!(!evidence.evidence_digest.is_empty());
    let evidence_repr = format!("{evidence:?}");
    assert!(!evidence_repr.contains("https://example.com"));
}

#[test]
fn activation_fails_closed_for_unbound_connector() {
    let mut registry = CapabilityRegistry::new();
    register_http_connector(&mut registry, "1.0.0");
    let request = activation_request("1.0.0", json!({ "base_url": "https://example.com" }));

    let failure = validate_connector_activation(&registry, LookupScope::PublicOnly, &[], &request)
        .expect_err("a connector with no application binding must not activate");

    assert_eq!(
        failure.errors[0].code,
        ConnectorActivationErrorCode::ConnectorUnbound
    );
}

#[test]
fn activation_fails_closed_for_unavailable_connector() {
    let mut registry = CapabilityRegistry::new();
    register_http_connector(&mut registry, "1.0.0");
    let bindings = vec![http_binding("^1.0.0")];
    // The host reports a version that was never registered in the registry.
    let request = activation_request("9.9.9", json!({ "base_url": "https://example.com" }));

    let failure =
        validate_connector_activation(&registry, LookupScope::PublicOnly, &bindings, &request)
            .expect_err("an unregistered installed connector version must not activate");

    assert_eq!(
        failure.errors[0].code,
        ConnectorActivationErrorCode::ConnectorUnavailable
    );
}

#[test]
fn activation_fails_closed_for_incompatible_installed_version() {
    let mut registry = CapabilityRegistry::new();
    register_http_connector(&mut registry, "1.0.0");
    register_http_connector(&mut registry, "2.0.0");
    let bindings = vec![http_binding("^1.0.0")];
    // 2.0.0 is registered, but outside the application's bound range.
    let request = activation_request("2.0.0", json!({ "base_url": "https://example.com" }));

    let failure =
        validate_connector_activation(&registry, LookupScope::PublicOnly, &bindings, &request)
            .expect_err("an installed version outside the bound range must not activate");

    assert_eq!(
        failure.errors[0].code,
        ConnectorActivationErrorCode::ConnectorVersionIncompatible
    );
}

#[test]
fn activation_fails_closed_for_unsupported_placement_target() {
    let mut registry = CapabilityRegistry::new();
    register_http_connector(&mut registry, "1.0.0");
    let bindings = vec![http_binding("^1.0.0")];
    let mut request = activation_request("1.0.0", json!({ "base_url": "https://example.com" }));
    // traverse.http's reference contract only supports Local and Cloud.
    request.placement_target = ExecutionTarget::Edge;

    let failure =
        validate_connector_activation(&registry, LookupScope::PublicOnly, &bindings, &request)
            .expect_err("an unsupported placement target must not activate");

    assert_eq!(
        failure.errors[0].code,
        ConnectorActivationErrorCode::ConnectorPlacementUnsupported
    );
}

#[test]
fn activation_fails_closed_for_missing_required_config_without_leaking_values() {
    let mut registry = CapabilityRegistry::new();
    register_http_connector(&mut registry, "1.0.0");
    let bindings = vec![http_binding("^1.0.0")];
    let request = activation_request("1.0.0", json!({ "unrelated_key": "top-secret-value" }));

    let failure =
        validate_connector_activation(&registry, LookupScope::PublicOnly, &bindings, &request)
            .expect_err("missing required configuration must not activate");

    assert_eq!(
        failure.errors[0].code,
        ConnectorActivationErrorCode::ConnectorUnconfigured
    );
    assert!(!failure.errors[0].message.contains("top-secret-value"));
}

#[test]
fn multiple_capabilities_share_one_compatible_activated_connector() {
    let mut registry = CapabilityRegistry::new();
    register_http_connector(&mut registry, "1.0.0");
    let bindings = vec![http_binding("^1.0.0")];
    let request = activation_request("1.0.0", json!({ "base_url": "https://example.com" }));

    // Two different capabilities in the same app both requiring traverse.http
    // independently validate activation against the same binding.
    let evidence_for_capability_a =
        validate_connector_activation(&registry, LookupScope::PublicOnly, &bindings, &request)
            .expect("first capability should activate the shared connector");
    let evidence_for_capability_b =
        validate_connector_activation(&registry, LookupScope::PublicOnly, &bindings, &request)
            .expect("second capability should activate the same shared connector");

    assert_eq!(evidence_for_capability_a, evidence_for_capability_b);
}

#[test]
fn detects_post_activation_version_drift() {
    let mut registry = CapabilityRegistry::new();
    register_http_connector(&mut registry, "1.0.0");
    register_http_connector(&mut registry, "1.1.0");
    let bindings = vec![http_binding(">=1.0.0")];

    let initial = validate_connector_activation(
        &registry,
        LookupScope::PublicOnly,
        &bindings,
        &activation_request("1.0.0", json!({ "base_url": "https://example.com" })),
    )
    .expect("initial activation should succeed");
    let later = validate_connector_activation(
        &registry,
        LookupScope::PublicOnly,
        &bindings,
        &activation_request("1.1.0", json!({ "base_url": "https://example.com" })),
    )
    .expect("host upgrading the installed connector should still activate");

    assert_eq!(
        detect_activation_drift(&initial, &later),
        ConnectorActivationDrift::VersionChanged
    );
}

#[test]
fn detects_post_activation_config_key_drift() {
    let mut registry = CapabilityRegistry::new();
    register_http_connector(&mut registry, "1.0.0");
    let bindings = vec![http_binding("^1.0.0")];

    let initial = validate_connector_activation(
        &registry,
        LookupScope::PublicOnly,
        &bindings,
        &activation_request("1.0.0", json!({ "base_url": "https://example.com" })),
    )
    .expect("initial activation should succeed");
    let later = validate_connector_activation(
        &registry,
        LookupScope::PublicOnly,
        &bindings,
        &activation_request(
            "1.0.0",
            json!({ "base_url": "https://example.com", "extra_key": "value" }),
        ),
    )
    .expect("host adding a config key should still activate");

    assert_eq!(
        detect_activation_drift(&initial, &later),
        ConnectorActivationDrift::ConfigKeysChanged
    );
}

#[test]
fn reports_no_drift_for_identical_repeated_activation() {
    let mut registry = CapabilityRegistry::new();
    register_http_connector(&mut registry, "1.0.0");
    let bindings = vec![http_binding("^1.0.0")];
    let request = activation_request("1.0.0", json!({ "base_url": "https://example.com" }));

    let first =
        validate_connector_activation(&registry, LookupScope::PublicOnly, &bindings, &request)
            .expect("first activation should succeed");
    let second =
        validate_connector_activation(&registry, LookupScope::PublicOnly, &bindings, &request)
            .expect("second activation should succeed");

    assert_eq!(
        detect_activation_drift(&first, &second),
        ConnectorActivationDrift::None
    );
}
