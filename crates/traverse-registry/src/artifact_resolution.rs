//! Activation-time executable artifact resolution (Traverse Spec 106,
//! ADR-0040, extending Spec 105/041/103).
//!
//! Deterministic, non-secret selection of the executable package that
//! satisfies a required capability contract at host activation time.
//! Registry never walks a package tree or loads artifacts itself -- the
//! caller (Traverse) supplies every candidate it has locally discovered as
//! [`ExecutableArtifactCandidate`]; this module only decides which one (if
//! any) is eligible and deterministically wins, and later whether a
//! previously recorded selection has drifted. Same "pure function, no
//! registry-side persistence" shape as `connector_activation.rs`.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use traverse_contracts::{ExecutionTarget, Lifecycle};

/// One executable package a host has locally discovered that may satisfy a
/// required capability contract. `execution_constraints` and `abi` are
/// opaque, non-secret strings this module compares for equality only (drift
/// detection) -- it does not interpret their contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableArtifactCandidate {
    pub package_id: String,
    pub package_version: String,
    pub contract_reference: String,
    pub digest: String,
    pub abi: String,
    pub lifecycle: Lifecycle,
    pub placement: Vec<ExecutionTarget>,
    pub execution_constraints: String,
}

/// A host activation request for one required capability contract.
/// `config_refs` names the host-private configuration references available
/// (never values -- the same non-secret discipline `connector_activation`
/// uses for `host_config`). `pin`, when present, MUST be honored exactly
/// (Spec 106 FR-003): an invalid pin fails rather than falling back to
/// another candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactResolutionRequest {
    pub contract_reference: String,
    pub placement_target: ExecutionTarget,
    pub config_refs: Vec<String>,
    pub pin: Option<ArtifactPin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ArtifactPin {
    pub package_id: String,
    pub package_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactResolutionErrorCode {
    /// No eligible candidate exists for the required contract (Spec 106
    /// acceptance scenario 1), or a pinned package id/version does not
    /// exist among the supplied candidates at all.
    ExecutableArtifactUnavailable,
    /// A pinned package id/version exists but fails eligibility (wrong
    /// contract, inactive, unsupported placement, or malformed digest/ABI)
    /// -- fails closed, never falls back to another candidate (ADR-0040).
    ExecutableArtifactIncompatible,
    /// A previously recorded selection no longer matches the current
    /// candidate's digest, lifecycle, ABI, placement, or execution
    /// constraints (Spec 106 FR-008).
    ActivationArtifactDrift,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactResolutionError {
    pub code: ArtifactResolutionErrorCode,
    pub contract_reference: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactResolutionFailure {
    pub errors: Vec<ArtifactResolutionError>,
}

/// Immutable, non-secret evidence of a successful resolution (Spec 106
/// FR-006). Captures enough of the selected candidate's own state
/// (`selected_lifecycle`/`selected_abi`/`selected_placement`/
/// `selected_execution_constraints`) for [`detect_artifact_drift`] to later
/// compare against the same package id/version's *current* state --
/// nothing here is re-derived from a live host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactResolutionEvidence {
    pub contract_reference: String,
    pub selected_package_id: String,
    pub selected_package_version: String,
    pub selected_digest: String,
    pub selected_lifecycle: Lifecycle,
    pub selected_abi: String,
    pub selected_placement: Vec<ExecutionTarget>,
    pub selected_execution_constraints: String,
    pub resolver_version: String,
    /// One human-readable line per candidate considered, recording why it
    /// was accepted or rejected -- a full audit trail, not just the winner.
    pub eligibility_decisions: Vec<String>,
    pub evidence_digest: String,
}

fn is_eligible(candidate: &ExecutableArtifactCandidate, request: &ArtifactResolutionRequest) -> Result<(), String> {
    if candidate.contract_reference != request.contract_reference {
        return Err(format!(
            "contract reference {} does not match required {}",
            candidate.contract_reference, request.contract_reference
        ));
    }
    if candidate.lifecycle != Lifecycle::Active {
        return Err(format!("lifecycle {:?} is not Active", candidate.lifecycle));
    }
    if candidate.digest.is_empty() {
        return Err("digest is empty".to_string());
    }
    if candidate.abi.is_empty() {
        return Err("abi is empty".to_string());
    }
    if !candidate.placement.contains(&request.placement_target) {
        return Err(format!(
            "placement {:?} does not include requested target {:?}",
            candidate.placement, request.placement_target
        ));
    }
    Ok(())
}

fn parse_package_version(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

/// Resolves the executable package that satisfies `request` from
/// `candidates`. An exact pin wins outright or fails closed
/// (`ExecutableArtifactIncompatible`/`ExecutableArtifactUnavailable`) --
/// never falls back to another package. Without a pin, selects the highest
/// package version among eligible candidates, then the lexicographically
/// lowest package id as a stable tie-break (Spec 106 FR-004). Candidates
/// whose `package_version` is not a valid `major.minor.patch` string are
/// treated as ineligible (unparseable version, deterministic ordering
/// cannot include them) rather than causing an error.
///
/// # Errors
///
/// Returns [`ArtifactResolutionFailure`] with `ExecutableArtifactUnavailable`
/// when no eligible candidate exists (or a pinned id/version is absent from
/// `candidates` entirely), or `ExecutableArtifactIncompatible` when a
/// pinned candidate exists but fails eligibility.
pub fn resolve_executable_artifact(
    request: &ArtifactResolutionRequest,
    candidates: &[ExecutableArtifactCandidate],
) -> Result<ArtifactResolutionEvidence, ArtifactResolutionFailure> {
    let mut eligibility_decisions = Vec::with_capacity(candidates.len());
    let mut eligible: Vec<&ExecutableArtifactCandidate> = Vec::new();
    for candidate in candidates {
        match is_eligible(candidate, request) {
            Ok(()) => {
                eligibility_decisions.push(format!(
                    "{}@{}: eligible",
                    candidate.package_id, candidate.package_version
                ));
                eligible.push(candidate);
            }
            Err(reason) => {
                eligibility_decisions.push(format!(
                    "{}@{}: rejected ({reason})",
                    candidate.package_id, candidate.package_version
                ));
            }
        }
    }

    let selected = if let Some(pin) = &request.pin {
        let pinned = candidates
            .iter()
            .find(|c| c.package_id == pin.package_id && c.package_version == pin.package_version);
        match pinned {
            None => {
                return Err(single_error(
                    ArtifactResolutionErrorCode::ExecutableArtifactUnavailable,
                    &request.contract_reference,
                    format!(
                        "pinned package {}@{} is not among the supplied candidates",
                        pin.package_id, pin.package_version
                    ),
                ));
            }
            Some(candidate) if !eligible.iter().any(|c| std::ptr::eq(*c, candidate)) => {
                return Err(single_error(
                    ArtifactResolutionErrorCode::ExecutableArtifactIncompatible,
                    &request.contract_reference,
                    format!(
                        "pinned package {}@{} is not eligible",
                        pin.package_id, pin.package_version
                    ),
                ));
            }
            Some(candidate) => candidate,
        }
    } else {
        let mut ordered: Vec<&ExecutableArtifactCandidate> = eligible
            .into_iter()
            .filter(|c| parse_package_version(&c.package_version).is_some())
            .collect();
        ordered.sort_by(|a, b| {
            let version_a = parse_package_version(&a.package_version).unwrap_or((0, 0, 0));
            let version_b = parse_package_version(&b.package_version).unwrap_or((0, 0, 0));
            version_b
                .cmp(&version_a)
                .then_with(|| a.package_id.cmp(&b.package_id))
        });
        match ordered.into_iter().next() {
            Some(candidate) => candidate,
            None => {
                return Err(single_error(
                    ArtifactResolutionErrorCode::ExecutableArtifactUnavailable,
                    &request.contract_reference,
                    format!(
                        "no eligible executable package satisfies contract {}",
                        request.contract_reference
                    ),
                ));
            }
        }
    };

    let resolver_version = env!("CARGO_PKG_VERSION").to_string();
    let evidence_digest = evidence_digest(
        &request.contract_reference,
        &selected.package_id,
        &selected.package_version,
        &selected.digest,
        &resolver_version,
    );

    Ok(ArtifactResolutionEvidence {
        contract_reference: request.contract_reference.clone(),
        selected_package_id: selected.package_id.clone(),
        selected_package_version: selected.package_version.clone(),
        selected_digest: selected.digest.clone(),
        selected_lifecycle: selected.lifecycle.clone(),
        selected_abi: selected.abi.clone(),
        selected_placement: selected.placement.clone(),
        selected_execution_constraints: selected.execution_constraints.clone(),
        resolver_version,
        eligibility_decisions,
        evidence_digest,
    })
}

/// Compares `previous` evidence against the *current* state of the same
/// package id/version in `candidates`. Never re-resolves or picks a
/// different package (Spec 106 FR-008) -- only ever compares the one
/// package `previous` already selected.
///
/// # Errors
///
/// Returns [`ArtifactResolutionFailure`] with `ActivationArtifactDrift` when
/// the previously selected package is no longer present in `candidates`, or
/// its digest, lifecycle, ABI, placement, or execution constraints differ
/// from what `previous` recorded.
pub fn detect_artifact_drift(
    previous: &ArtifactResolutionEvidence,
    candidates: &[ExecutableArtifactCandidate],
) -> Result<(), ArtifactResolutionFailure> {
    let current = candidates.iter().find(|c| {
        c.package_id == previous.selected_package_id
            && c.package_version == previous.selected_package_version
    });

    let Some(current) = current else {
        return Err(single_error(
            ArtifactResolutionErrorCode::ActivationArtifactDrift,
            &previous.contract_reference,
            format!(
                "previously selected package {}@{} is no longer present",
                previous.selected_package_id, previous.selected_package_version
            ),
        ));
    };

    if current.digest != previous.selected_digest
        || current.lifecycle != previous.selected_lifecycle
        || current.abi != previous.selected_abi
        || current.placement != previous.selected_placement
        || current.execution_constraints != previous.selected_execution_constraints
    {
        return Err(single_error(
            ArtifactResolutionErrorCode::ActivationArtifactDrift,
            &previous.contract_reference,
            format!(
                "package {}@{} drifted from its recorded activation evidence",
                previous.selected_package_id, previous.selected_package_version
            ),
        ));
    }

    Ok(())
}

fn evidence_digest(
    contract_reference: &str,
    package_id: &str,
    package_version: &str,
    digest: &str,
    resolver_version: &str,
) -> String {
    let value = serde_json::json!({
        "contract_reference": contract_reference,
        "package_id": package_id,
        "package_version": package_version,
        "digest": digest,
        "resolver_version": resolver_version,
    });
    let computed = Sha256::digest(value.to_string().as_bytes());
    let mut output = String::with_capacity(computed.len() * 2 + 7);
    output.push_str("sha256:");
    for byte in computed {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn single_error(
    code: ArtifactResolutionErrorCode,
    contract_reference: &str,
    message: String,
) -> ArtifactResolutionFailure {
    ArtifactResolutionFailure {
        errors: vec![ArtifactResolutionError {
            code,
            contract_reference: contract_reference.to_string(),
            message,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(
        package_id: &str,
        package_version: &str,
        contract_reference: &str,
        lifecycle: Lifecycle,
        placement: Vec<ExecutionTarget>,
    ) -> ExecutableArtifactCandidate {
        ExecutableArtifactCandidate {
            package_id: package_id.to_string(),
            package_version: package_version.to_string(),
            contract_reference: contract_reference.to_string(),
            digest: "sha256:abc".to_string(),
            abi: "wasi-p1".to_string(),
            lifecycle,
            placement,
            execution_constraints: "network:forbidden".to_string(),
        }
    }

    fn request(contract_reference: &str, pin: Option<ArtifactPin>) -> ArtifactResolutionRequest {
        ArtifactResolutionRequest {
            contract_reference: contract_reference.to_string(),
            placement_target: ExecutionTarget::Cloud,
            config_refs: vec!["default-config".to_string()],
            pin,
        }
    }

    #[test]
    fn zero_candidates_fails_unavailable() {
        let result = resolve_executable_artifact(&request("core.example", None), &[]);
        let failure = result.expect_err("must fail closed with no candidates");
        assert_eq!(
            failure.errors[0].code,
            ArtifactResolutionErrorCode::ExecutableArtifactUnavailable
        );
    }

    #[test]
    fn one_eligible_candidate_resolves() {
        let candidates = vec![candidate(
            "pkg-a",
            "1.0.0",
            "core.example",
            Lifecycle::Active,
            vec![ExecutionTarget::Cloud],
        )];
        let evidence = resolve_executable_artifact(&request("core.example", None), &candidates)
            .expect("must resolve");
        assert_eq!(evidence.selected_package_id, "pkg-a");
        assert_eq!(evidence.selected_package_version, "1.0.0");
        assert_eq!(evidence.eligibility_decisions.len(), 1);
        assert!(evidence.eligibility_decisions[0].contains("eligible"));
    }

    #[test]
    fn multiple_candidates_select_highest_version_then_lowest_id_tiebreak() {
        let candidates = vec![
            candidate("pkg-b", "1.0.0", "core.example", Lifecycle::Active, vec![ExecutionTarget::Cloud]),
            candidate("pkg-a", "2.0.0", "core.example", Lifecycle::Active, vec![ExecutionTarget::Cloud]),
            candidate("pkg-c", "2.0.0", "core.example", Lifecycle::Active, vec![ExecutionTarget::Cloud]),
        ];
        let evidence = resolve_executable_artifact(&request("core.example", None), &candidates)
            .expect("must resolve");
        // Highest version (2.0.0) wins over 1.0.0; among the 2.0.0 tie,
        // lexicographically lowest package id (pkg-a) wins over pkg-c.
        assert_eq!(evidence.selected_package_id, "pkg-a");
        assert_eq!(evidence.selected_package_version, "2.0.0");
        assert_eq!(evidence.eligibility_decisions.len(), 3);
    }

    #[test]
    fn valid_pin_selects_exact_compatible_package() {
        let candidates = vec![
            candidate("pkg-a", "1.0.0", "core.example", Lifecycle::Active, vec![ExecutionTarget::Cloud]),
            candidate("pkg-b", "9.0.0", "core.example", Lifecycle::Active, vec![ExecutionTarget::Cloud]),
        ];
        let pin = ArtifactPin { package_id: "pkg-a".to_string(), package_version: "1.0.0".to_string() };
        let evidence = resolve_executable_artifact(&request("core.example", Some(pin)), &candidates)
            .expect("must resolve to the pinned package");
        assert_eq!(evidence.selected_package_id, "pkg-a");
        assert_eq!(evidence.selected_package_version, "1.0.0");
    }

    #[test]
    fn invalid_pin_fails_without_falling_back() {
        let candidates = vec![
            candidate("pkg-a", "1.0.0", "core.example", Lifecycle::Deprecated, vec![ExecutionTarget::Cloud]),
            candidate("pkg-b", "1.0.0", "core.example", Lifecycle::Active, vec![ExecutionTarget::Cloud]),
        ];
        let pin = ArtifactPin { package_id: "pkg-a".to_string(), package_version: "1.0.0".to_string() };
        let result = resolve_executable_artifact(&request("core.example", Some(pin)), &candidates);
        let failure = result.expect_err("an incompatible pin must not fall back to pkg-b");
        assert_eq!(
            failure.errors[0].code,
            ArtifactResolutionErrorCode::ExecutableArtifactIncompatible
        );
    }

    #[test]
    fn pin_absent_from_candidates_fails_unavailable() {
        let candidates = vec![candidate("pkg-a", "1.0.0", "core.example", Lifecycle::Active, vec![ExecutionTarget::Cloud])];
        let pin = ArtifactPin { package_id: "pkg-z".to_string(), package_version: "9.9.9".to_string() };
        let result = resolve_executable_artifact(&request("core.example", Some(pin)), &candidates);
        let failure = result.expect_err("a pin naming a nonexistent package must fail");
        assert_eq!(
            failure.errors[0].code,
            ArtifactResolutionErrorCode::ExecutableArtifactUnavailable
        );
    }

    #[test]
    fn incompatible_candidates_are_rejected() {
        let candidates = vec![
            candidate("pkg-inactive", "1.0.0", "core.example", Lifecycle::Deprecated, vec![ExecutionTarget::Cloud]),
            candidate("pkg-wrong-placement", "1.0.0", "core.example", Lifecycle::Active, vec![ExecutionTarget::Browser]),
            candidate("pkg-wrong-contract", "1.0.0", "core.other", Lifecycle::Active, vec![ExecutionTarget::Cloud]),
        ];
        let result = resolve_executable_artifact(&request("core.example", None), &candidates);
        let failure = result.expect_err("no candidate is eligible");
        assert_eq!(
            failure.errors[0].code,
            ArtifactResolutionErrorCode::ExecutableArtifactUnavailable
        );
    }

    #[test]
    fn no_drift_when_candidate_state_is_unchanged() {
        let original = candidate("pkg-a", "1.0.0", "core.example", Lifecycle::Active, vec![ExecutionTarget::Cloud]);
        let evidence = resolve_executable_artifact(&request("core.example", None), &[original.clone()])
            .expect("must resolve");
        assert!(detect_artifact_drift(&evidence, &[original]).is_ok());
    }

    #[test]
    fn digest_change_is_detected_as_drift() {
        let original = candidate("pkg-a", "1.0.0", "core.example", Lifecycle::Active, vec![ExecutionTarget::Cloud]);
        let evidence = resolve_executable_artifact(&request("core.example", None), &[original.clone()])
            .expect("must resolve");
        let mut changed = original;
        changed.digest = "sha256:different".to_string();
        let failure = detect_artifact_drift(&evidence, &[changed]).expect_err("digest change must drift");
        assert_eq!(failure.errors[0].code, ArtifactResolutionErrorCode::ActivationArtifactDrift);
    }

    #[test]
    fn lifecycle_change_is_detected_as_drift() {
        let original = candidate("pkg-a", "1.0.0", "core.example", Lifecycle::Active, vec![ExecutionTarget::Cloud]);
        let evidence = resolve_executable_artifact(&request("core.example", None), &[original.clone()])
            .expect("must resolve");
        let mut changed = original;
        changed.lifecycle = Lifecycle::Deprecated;
        let failure = detect_artifact_drift(&evidence, &[changed]).expect_err("lifecycle change must drift");
        assert_eq!(failure.errors[0].code, ArtifactResolutionErrorCode::ActivationArtifactDrift);
    }

    #[test]
    fn placement_change_is_detected_as_drift() {
        let original = candidate("pkg-a", "1.0.0", "core.example", Lifecycle::Active, vec![ExecutionTarget::Cloud]);
        let evidence = resolve_executable_artifact(&request("core.example", None), &[original.clone()])
            .expect("must resolve");
        let mut changed = original;
        changed.placement = vec![ExecutionTarget::Cloud, ExecutionTarget::Edge];
        let failure = detect_artifact_drift(&evidence, &[changed]).expect_err("placement change must drift");
        assert_eq!(failure.errors[0].code, ArtifactResolutionErrorCode::ActivationArtifactDrift);
    }

    #[test]
    fn constraints_change_is_detected_as_drift() {
        let original = candidate("pkg-a", "1.0.0", "core.example", Lifecycle::Active, vec![ExecutionTarget::Cloud]);
        let evidence = resolve_executable_artifact(&request("core.example", None), &[original.clone()])
            .expect("must resolve");
        let mut changed = original;
        changed.execution_constraints = "network:allowed".to_string();
        let failure = detect_artifact_drift(&evidence, &[changed]).expect_err("constraints change must drift");
        assert_eq!(failure.errors[0].code, ArtifactResolutionErrorCode::ActivationArtifactDrift);
    }

    #[test]
    fn removed_candidate_is_detected_as_drift() {
        let original = candidate("pkg-a", "1.0.0", "core.example", Lifecycle::Active, vec![ExecutionTarget::Cloud]);
        let evidence = resolve_executable_artifact(&request("core.example", None), &[original])
            .expect("must resolve");
        let failure = detect_artifact_drift(&evidence, &[]).expect_err("a removed package must drift");
        assert_eq!(failure.errors[0].code, ArtifactResolutionErrorCode::ActivationArtifactDrift);
    }

    #[test]
    fn evidence_is_deterministic_for_identical_inputs() {
        let candidates = vec![candidate("pkg-a", "1.0.0", "core.example", Lifecycle::Active, vec![ExecutionTarget::Cloud])];
        let first = resolve_executable_artifact(&request("core.example", None), &candidates).expect("must resolve");
        let second = resolve_executable_artifact(&request("core.example", None), &candidates).expect("must resolve");
        assert_eq!(first, second);
        assert_eq!(first.evidence_digest, second.evidence_digest);
    }

    #[test]
    fn evidence_never_carries_config_ref_values() {
        // config_refs on the request are host-private reference names; the
        // evidence must never echo them back (non-secret discipline).
        let candidates = vec![candidate("pkg-a", "1.0.0", "core.example", Lifecycle::Active, vec![ExecutionTarget::Cloud])];
        let mut req = request("core.example", None);
        req.config_refs = vec!["secret-looking-ref-name".to_string()];
        let evidence = resolve_executable_artifact(&req, &candidates).expect("must resolve");
        let debug = format!("{evidence:?}");
        assert!(!debug.contains("secret-looking-ref-name"));
    }
}
