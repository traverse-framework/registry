//! Host activation validation for Spec 103 application connector bindings
//! (ADR-0039, extending Spec 039).
//!
//! Static bundle validation (`application_manifest.rs`) only ever reads
//! portable, non-secret bundle and registry data. This module performs the
//! complementary host-side check that runs once a concrete connector
//! implementation and its private configuration are available. It never
//! copies a configuration *value* into its evidence or errors — only
//! configuration key *names* ever leave this module.

use semver::{Version, VersionReq};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt::Write;
use traverse_contracts::ExecutionTarget;

use crate::{ApplicationConnectorBinding, CapabilityRegistry, LookupScope};

/// The identity and concrete version a host reports as installed for a
/// connector id, independent of the abstract semver range the application
/// bound against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledConnector {
    pub connector_id: String,
    pub version: String,
}

/// A host-side activation request. `host_config` is private: only the
/// *names* of the keys it carries are ever read out of this module, never
/// values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorActivationRequest {
    pub connector_id: String,
    pub installed: InstalledConnector,
    pub placement_target: ExecutionTarget,
    pub host_config: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorActivationErrorCode {
    /// The application declares no binding for this connector id.
    ConnectorUnbound,
    /// The host-reported installed connector is not a registered version of
    /// this connector id.
    ConnectorUnavailable,
    /// The installed connector's version does not satisfy the binding's
    /// declared semver range.
    ConnectorVersionIncompatible,
    /// The connector's contract does not support the requested placement
    /// target.
    ConnectorPlacementUnsupported,
    /// A required configuration key (per the connector contract's
    /// `required_config_schema`) is absent from the host-private config.
    ConnectorUnconfigured,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorActivationError {
    pub code: ConnectorActivationErrorCode,
    pub connector_id: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorActivationFailure {
    pub errors: Vec<ConnectorActivationError>,
}

/// Deterministic, non-secret proof that a connector was activated. Records
/// only config *key names*, never values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorActivationEvidence {
    pub connector_id: String,
    pub resolved_version: String,
    pub placement_target: ExecutionTarget,
    pub config_keys_present: Vec<String>,
    pub evidence_digest: String,
}

/// Whether a later activation attempt for the same connector binding
/// diverged from a previously recorded [`ConnectorActivationEvidence`].
/// Computed only from evidence (key names, resolved version, placement),
/// never from raw host configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorActivationDrift {
    None,
    VersionChanged,
    PlacementChanged,
    ConfigKeysChanged,
}

/// Validates and activates a Spec 103 connector binding against a
/// host-reported installed connector, its registered contract, a placement
/// target, and private configuration.
///
/// Multiple capabilities that declare the same connector requirement may
/// call this with the same `request.connector_id`; each call independently
/// re-validates and produces identical evidence for the same inputs, so one
/// compatible activated connector can be safely shared across them.
///
/// Fails closed: any single check failing returns
/// [`ConnectorActivationFailure`] and produces no evidence.
///
/// # Errors
///
/// Returns [`ConnectorActivationFailure`] when the connector id has no
/// application binding (`ConnectorUnbound`), the installed connector is not
/// a registered version of that connector id (`ConnectorUnavailable`), the
/// installed version does not satisfy the binding's range
/// (`ConnectorVersionIncompatible`), the connector's contract does not
/// support the requested placement target
/// (`ConnectorPlacementUnsupported`), or a required configuration key is
/// absent from `request.host_config` (`ConnectorUnconfigured`).
pub fn validate_connector_activation(
    registry: &CapabilityRegistry,
    lookup_scope: LookupScope,
    bindings: &[ApplicationConnectorBinding],
    request: &ConnectorActivationRequest,
) -> Result<ConnectorActivationEvidence, ConnectorActivationFailure> {
    let Some(binding) = bindings
        .iter()
        .find(|binding| binding.connector_id == request.connector_id)
    else {
        return Err(single_error(
            ConnectorActivationErrorCode::ConnectorUnbound,
            &request.connector_id,
            format!(
                "connector {} has no application binding",
                request.connector_id
            ),
        ));
    };

    let registered_versions =
        registry.discover_connectors(lookup_scope, &request.connector_id, "*");
    let Some(record) = registered_versions
        .iter()
        .find(|record| record.version == request.installed.version)
    else {
        return Err(single_error(
            ConnectorActivationErrorCode::ConnectorUnavailable,
            &request.connector_id,
            format!(
                "installed connector {} version {} is not a registered connector",
                request.connector_id, request.installed.version
            ),
        ));
    };

    let version_matches = VersionReq::parse(&binding.version_range)
        .ok()
        .zip(Version::parse(&record.version).ok())
        .is_some_and(|(range, version)| range.matches(&version));
    if !version_matches {
        return Err(single_error(
            ConnectorActivationErrorCode::ConnectorVersionIncompatible,
            &request.connector_id,
            format!(
                "installed connector version {} for {} does not satisfy bound range {}",
                record.version, request.connector_id, binding.version_range
            ),
        ));
    }

    if !record
        .supported_placement_targets
        .contains(&request.placement_target)
    {
        return Err(single_error(
            ConnectorActivationErrorCode::ConnectorPlacementUnsupported,
            &request.connector_id,
            format!(
                "connector {} does not support placement target {:?}",
                request.connector_id, request.placement_target
            ),
        ));
    }

    let missing_keys =
        missing_required_config_keys(&record.required_config_schema, &request.host_config);
    if !missing_keys.is_empty() {
        return Err(single_error(
            ConnectorActivationErrorCode::ConnectorUnconfigured,
            &request.connector_id,
            format!(
                "connector {} is missing required configuration keys: {}",
                request.connector_id,
                missing_keys.join(", ")
            ),
        ));
    }

    let mut config_keys_present = present_config_keys(&request.host_config);
    config_keys_present.sort();

    let evidence_digest = activation_evidence_digest(
        &request.connector_id,
        &record.version,
        &request.placement_target,
        &config_keys_present,
    );

    Ok(ConnectorActivationEvidence {
        connector_id: request.connector_id.clone(),
        resolved_version: record.version.clone(),
        placement_target: request.placement_target.clone(),
        config_keys_present,
        evidence_digest,
    })
}

/// Compares two activation evidence snapshots for the same connector
/// binding to detect host-side drift after the original activation.
#[must_use]
pub fn detect_activation_drift(
    previous: &ConnectorActivationEvidence,
    current: &ConnectorActivationEvidence,
) -> ConnectorActivationDrift {
    if previous.resolved_version != current.resolved_version {
        return ConnectorActivationDrift::VersionChanged;
    }
    if previous.placement_target != current.placement_target {
        return ConnectorActivationDrift::PlacementChanged;
    }
    if previous.config_keys_present != current.config_keys_present {
        return ConnectorActivationDrift::ConfigKeysChanged;
    }
    ConnectorActivationDrift::None
}

fn required_config_keys(schema: &Value) -> Vec<String> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn present_config_keys(host_config: &Value) -> Vec<String> {
    host_config
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

fn missing_required_config_keys(schema: &Value, host_config: &Value) -> Vec<String> {
    let present = host_config.as_object();
    required_config_keys(schema)
        .into_iter()
        .filter(|key| !present.is_some_and(|map| map.contains_key(key)))
        .collect()
}

fn activation_evidence_digest(
    connector_id: &str,
    resolved_version: &str,
    placement_target: &ExecutionTarget,
    config_keys_present: &[String],
) -> String {
    let value = serde_json::json!({
        "connector_id": connector_id,
        "resolved_version": resolved_version,
        "placement_target": placement_target,
        "config_keys_present": config_keys_present,
    });
    let digest = Sha256::digest(value.to_string().as_bytes());
    let mut output = String::with_capacity(digest.len() * 2 + 7);
    output.push_str("sha256:");
    for byte in digest {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn single_error(
    code: ConnectorActivationErrorCode,
    connector_id: &str,
    message: String,
) -> ConnectorActivationFailure {
    ConnectorActivationFailure {
        errors: vec![ConnectorActivationError {
            code,
            connector_id: connector_id.to_string(),
            message,
        }],
    }
}
