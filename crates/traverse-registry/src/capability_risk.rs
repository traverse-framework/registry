//! Spec `024-capability-risk-classification-adoption` FR-003 / FR-004:
//! resolve every published capability version's risk classification **through
//! `traverse-contracts`** -- the crate that owns `traverse` Spec 109's
//! `RiskMetadata`, `is_automatic_eligible`, and `default_risk_metadata` -- so
//! the catalog / index build pipelines (`gather_catalog_data.py`,
//! `build_index.py`, the `no_std` `catalog-builder`) never re-derive the
//! automatic-eligibility rule themselves and never drift from the crate if it
//! changes (it already grew a fourth condition, `!idempotency_required`).
//!
//! A contract that declares a `risk` block gets `risk_source: "declared"`; a
//! contract with none resolves via `traverse_contracts::default_risk_metadata`
//! (the canonical pre-109 conservative fallback) and gets
//! `risk_source: "conservative_default"` -- mirroring exactly what the runtime
//! does for such a contract, so a consumer filtering `is_automatic_eligible`
//! never has to special-case a missing block.

use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;
use traverse_contracts::{RiskMetadata, default_risk_metadata, is_automatic_eligible};

/// Where a resolved [`CapabilityRiskProjection::risk`] came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskSource {
    /// The contract declared its own `risk` block.
    Declared,
    /// The contract declared none; `traverse_contracts::default_risk_metadata`
    /// was used.
    ConservativeDefault,
}

/// The public risk projection for one capability version, as embedded in
/// `catalog.json` / `index.json` capability entries (Spec 024 FR-003).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CapabilityRiskProjection {
    /// `<namespace>/<id>@<version>` -- matches `catalog-builder`'s
    /// `capability_reference` and `build_index.py`'s entry identity.
    pub reference: String,
    /// The `RiskMetadata` as declared, or `default_risk_metadata()` when
    /// absent, serialized to JSON (`snake_case` enums; `data_flow` always
    /// present).
    pub risk: Value,
    /// `traverse_contracts::is_automatic_eligible` applied to `risk`.
    pub is_automatic_eligible: bool,
    pub risk_source: RiskSource,
}

/// A contract that could not be resolved (unreadable, unparseable, missing
/// identity, or a malformed declared `risk` block). Fatal for the pipeline --
/// a silently dropped record is the same failure class an unverifiable record
/// reaching a consumer is.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CapabilityRiskError {
    pub path: String,
    pub message: String,
}

/// Resolve one contract's risk projection from its parsed JSON.
///
/// # Errors
/// Returns [`CapabilityRiskError`] when the contract lacks `namespace`/`id`/
/// `version`, or declares a `risk` block that does not deserialize as
/// `traverse_contracts::RiskMetadata`.
pub fn resolve_projection(
    contract: &Value,
    path_for_errors: &str,
) -> Result<CapabilityRiskProjection, CapabilityRiskError> {
    let err = |message: String| CapabilityRiskError {
        path: path_for_errors.to_string(),
        message,
    };

    let namespace = contract
        .get("namespace")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let id = contract
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let version = contract
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if namespace.is_empty() || id.is_empty() || version.is_empty() {
        return Err(err("contract is missing namespace/id/version".to_string()));
    }

    let (risk, risk_source) = match contract.get("risk") {
        Some(value) if !value.is_null() => {
            let parsed: RiskMetadata = serde_json::from_value(value.clone()).map_err(|e| {
                err(format!(
                    "declared `risk` block does not deserialize as RiskMetadata: {e}"
                ))
            })?;
            (parsed, RiskSource::Declared)
        }
        _ => (default_risk_metadata(), RiskSource::ConservativeDefault),
    };

    let risk_value = serde_json::to_value(&risk)
        .map_err(|e| err(format!("unable to serialize resolved RiskMetadata: {e}")))?;

    Ok(CapabilityRiskProjection {
        reference: format!("{namespace}/{id}@{version}"),
        is_automatic_eligible: is_automatic_eligible(&risk),
        risk: risk_value,
        risk_source,
    })
}

/// Walk `<root>/capabilities/<namespace>/<id>/<version>/contract.json` and
/// resolve every version (deprecated included -- the catalog/index pipelines
/// carry deprecated entries too). Sorted by `reference` for deterministic
/// output.
///
/// # Errors
/// Returns every [`CapabilityRiskError`] encountered; an empty `Err` vec is
/// never returned (an all-ok walk is `Ok`).
pub fn resolve_tree(
    root: &Path,
) -> Result<Vec<CapabilityRiskProjection>, Vec<CapabilityRiskError>> {
    let capabilities_dir = root.join("capabilities");
    let mut projections: Vec<CapabilityRiskProjection> = Vec::new();
    let mut errors: Vec<CapabilityRiskError> = Vec::new();

    let mut contract_paths: Vec<_> = Vec::new();
    collect_contract_paths(&capabilities_dir, 0, &mut contract_paths, &mut errors);
    contract_paths.sort();

    for path in contract_paths {
        let display = path.to_string_lossy().into_owned();
        let raw = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) => {
                errors.push(CapabilityRiskError {
                    path: display,
                    message: format!("unreadable: {e}"),
                });
                continue;
            }
        };
        let parsed: Value = match serde_json::from_slice(&raw) {
            Ok(value) => value,
            Err(e) => {
                errors.push(CapabilityRiskError {
                    path: display,
                    message: format!("invalid JSON: {e}"),
                });
                continue;
            }
        };
        match resolve_projection(&parsed, &display) {
            Ok(projection) => projections.push(projection),
            Err(e) => errors.push(e),
        }
    }

    if errors.is_empty() {
        projections.sort_by(|a, b| a.reference.cmp(&b.reference));
        Ok(projections)
    } else {
        errors.sort();
        Err(errors)
    }
}

/// `capabilities/<namespace>/<id>/<version>/contract.json` is exactly three
/// directory levels below `capabilities/`; don't recurse arbitrarily.
fn collect_contract_paths(
    dir: &Path,
    depth: usize,
    out: &mut Vec<std::path::PathBuf>,
    errors: &mut Vec<CapabilityRiskError>,
) {
    if depth == 3 {
        let contract = dir.join("contract.json");
        if contract.is_file() {
            out.push(contract);
        }
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            // A missing capabilities/ dir at depth 0 is just an empty tree.
            if !(depth == 0 && e.kind() == std::io::ErrorKind::NotFound) {
                errors.push(CapabilityRiskError {
                    path: dir.to_string_lossy().into_owned(),
                    message: format!("unable to read directory: {e}"),
                });
            }
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_contract_paths(&path, depth + 1, out, errors);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    #![allow(clippy::needless_pass_by_value)]

    use super::*;
    use serde_json::json;

    fn contract_with_risk(risk: Value) -> Value {
        json!({
            "namespace": "core",
            "id": "core.thing",
            "version": "1.0.0",
            "risk": risk,
        })
    }

    fn eligible_risk() -> Value {
        json!({
            "effect_class": "pure_read",
            "determinism_class": "deterministic",
            "data_flow": { "egress_policy": "denied" },
            "reliability": {
                "idempotency_required": false,
                "retryable": true,
                "compensation_available": false
            }
        })
    }

    fn project(contract: &Value) -> CapabilityRiskProjection {
        resolve_projection(contract, "p").expect("projection should resolve")
    }

    #[test]
    fn declared_eligible_risk_projects_true_and_declared_source() {
        let projection = project(&contract_with_risk(eligible_risk()));
        assert_eq!(projection.reference, "core/core.thing@1.0.0");
        assert!(projection.is_automatic_eligible);
        assert_eq!(projection.risk_source, RiskSource::Declared);
        // data_flow is materialized even though the input only gave egress_policy.
        assert_eq!(
            projection.risk["data_flow"]["egress_policy"],
            json!("denied")
        );
        assert_eq!(projection.risk["effect_class"], json!("pure_read"));
    }

    #[test]
    fn each_dimension_flip_makes_it_ineligible_via_the_crate_rule() {
        for (field, value) in [
            ("effect_class", json!("state_write")),
            ("determinism_class", json!("model_derived")),
        ] {
            let mut risk = eligible_risk();
            risk[field] = value;
            assert!(
                !project(&contract_with_risk(risk)).is_automatic_eligible,
                "flipping {field} must make it ineligible"
            );
        }

        let mut risk = eligible_risk();
        risk["data_flow"]["egress_policy"] = json!({ "allowed_connectors": ["smtp"] });
        assert!(!project(&contract_with_risk(risk)).is_automatic_eligible);

        let mut risk = eligible_risk();
        risk["reliability"]["idempotency_required"] = json!(true);
        assert!(!project(&contract_with_risk(risk)).is_automatic_eligible);
    }

    #[test]
    fn absent_risk_resolves_to_conservative_default_and_false() {
        let projection = project(&json!({
            "namespace": "core", "id": "core.legacy", "version": "2.0.0"
        }));
        assert_eq!(projection.risk_source, RiskSource::ConservativeDefault);
        assert!(!projection.is_automatic_eligible);
        // Mirrors traverse_contracts::default_risk_metadata().
        assert_eq!(
            projection.risk["effect_class"],
            json!("irreversible_effect")
        );
        assert_eq!(projection.risk["determinism_class"], json!("model_derived"));
        assert_eq!(
            projection.risk["reliability"]["idempotency_required"],
            json!(true)
        );
    }

    #[test]
    fn null_risk_is_treated_as_absent() {
        let mut contract = contract_with_risk(Value::Null);
        contract["risk"] = Value::Null;
        assert_eq!(
            project(&contract).risk_source,
            RiskSource::ConservativeDefault
        );
    }

    #[test]
    fn malformed_declared_risk_is_a_hard_error() {
        let risk = json!({
            "effect_class": "read_only",
            "determinism_class": "deterministic",
            "reliability": {
                "idempotency_required": false, "retryable": true, "compensation_available": false
            }
        });
        let err = resolve_projection(&contract_with_risk(risk), "some/path")
            .expect_err("an unknown effect_class must be rejected");
        assert_eq!(err.path, "some/path");
        assert!(err.message.contains("does not deserialize"));
    }

    #[test]
    fn missing_identity_is_an_error() {
        let err = resolve_projection(&json!({ "risk": eligible_risk() }), "p")
            .expect_err("a contract with no identity must be rejected");
        assert!(err.message.contains("namespace/id/version"));
    }

    #[test]
    fn resolve_tree_walks_three_levels_and_sorts() {
        let dir = std::env::temp_dir().join(format!("cap-risk-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let make = |ns: &str, id: &str, ver: &str, risk: Option<Value>| {
            let p = dir.join("capabilities").join(ns).join(id).join(ver);
            fs::create_dir_all(&p).expect("mkdir");
            let mut body = json!({ "namespace": ns, "id": id, "version": ver });
            if let Some(risk) = risk {
                body["risk"] = risk;
            }
            fs::write(
                p.join("contract.json"),
                serde_json::to_vec(&body).expect("serialize"),
            )
            .expect("write");
        };
        make("z", "z.one", "1.0.0", Some(eligible_risk()));
        make("a", "a.two", "1.0.0", None);

        let out = resolve_tree(&dir).expect("tree resolves");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].reference, "a/a.two@1.0.0");
        assert_eq!(out[0].risk_source, RiskSource::ConservativeDefault);
        assert_eq!(out[1].reference, "z/z.one@1.0.0");
        assert!(out[1].is_automatic_eligible);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_tree_missing_capabilities_dir_is_empty_ok() {
        let dir = std::env::temp_dir().join(format!("cap-risk-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        assert_eq!(resolve_tree(&dir).expect("empty tree resolves"), Vec::new());
        let _ = fs::remove_dir_all(&dir);
    }
}
