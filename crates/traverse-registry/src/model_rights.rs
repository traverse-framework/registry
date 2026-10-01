//! `specs/026-model-rights-compliance` (registry decision-log entry 127):
//! typed view of a capability's `ai` declaration and its per-model rights
//! record, as projected verbatim into the public `index.json`, plus the
//! index's derived `status` and `model_usage` fields (FR-013/FR-014/FR-015).
//!
//! These types describe what the publisher declared and the Registry signed.
//! They are not a legal certification.

use serde::{Deserialize, Deserializer, Serialize};

/// Spec 001 FR-017 `ai` object: `model_backed: true` defines an agent.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PublicAiDeclaration {
    pub model_backed: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<PublicModelReference>,
}

/// One `ai.models` entry: the legacy identifier string (grandfathered on
/// contracts published before decision-log entry 124) or an object
/// `ModelRef`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum PublicModelReference {
    Legacy(String),
    Object(Box<PublicModelRef>),
}

impl PublicModelReference {
    /// The model identifier regardless of shape.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Legacy(id) => id,
            Self::Object(model_ref) => &model_ref.id,
        }
    }
}

/// Spec 001 FR-017 object `ModelRef`, extended by spec 026. The spec 026
/// fields are optional here because references published before spec 026
/// do not carry them. CI requires them on every newly added model-backed
/// contract.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PublicModelRef {
    pub id: String,
    pub spdx_expression: String,
    pub attribution_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub huggingface_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copyright: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commercial_use: Option<ModelRight>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redistribution: Option<ModelRight>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derivatives: Option<ModelRight>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub license_files: Vec<PinnedEvidenceFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notice_files: Vec<PinnedEvidenceFile>,
    /// `None`: not declared (pre-026 reference). Spec 026 requires the key
    /// on new references, as JSON `null` ([`ModelDerivationDeclaration::Verbatim`])
    /// or an object ([`ModelDerivationDeclaration::Converted`]).
    #[serde(
        default,
        deserialize_with = "declared_derivation",
        skip_serializing_if = "Option::is_none"
    )]
    pub derivation: Option<ModelDerivationDeclaration>,
    /// Absent means "not stated by the publisher", never "none" (FR-007).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_obligations: Option<Vec<ModelDataObligation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<ModelRightsVerification>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rights_change: Option<ModelRightsChange>,
}

impl PublicModelRef {
    /// Spec 026 "Derived usage class", computed only from the three rights
    /// enums. `None` when any of them is undeclared.
    #[must_use]
    pub fn usage_class(&self) -> Option<ModelUsageClass> {
        let (Some(commercial), Some(redistribution), Some(derivatives)) =
            (self.commercial_use, self.redistribution, self.derivatives)
        else {
            return None;
        };
        let all_allowed = [commercial, redistribution, derivatives]
            .iter()
            .all(|right| *right == ModelRight::Allowed);
        Some(if all_allowed {
            ModelUsageClass::Unrestricted
        } else if commercial == ModelRight::Forbidden {
            ModelUsageClass::EvaluationOnly
        } else {
            ModelUsageClass::Conditional
        })
    }
}

/// A present `derivation` key, including an explicit `null`, is a
/// declaration; only an absent key deserializes to `None` (via `default`).
fn declared_derivation<'de, D>(
    deserializer: D,
) -> Result<Option<ModelDerivationDeclaration>, D::Error>
where
    D: Deserializer<'de>,
{
    ModelDerivationDeclaration::deserialize(deserializer).map(Some)
}

/// FR-006: how the shipped weights relate to the pinned upstream revision.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ModelDerivationDeclaration {
    Converted(ModelDerivation),
    /// Serialized as JSON `null`: shipped byte-for-byte from upstream.
    Verbatim,
}

/// Spec 025 rights vocabulary. Consumers must treat anything other than
/// `Allowed` as not permitted (deny by default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRight {
    Allowed,
    Forbidden,
    Conditional,
    Unknown,
}

/// A LICENSE or NOTICE file pinned to a Registry Release asset (FR-005).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PinnedEvidenceFile {
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ModelDerivation {
    pub transformations: Vec<ModelTransformation>,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_url: Option<String>,
    pub converted_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelTransformation {
    Quantize,
    FormatConvert,
    Prune,
    Distill,
    FineTune,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ModelDataObligation {
    pub dataset: String,
    pub kind: ModelDataKind,
    pub obligation: String,
    pub source_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spdx_expression: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelDataKind {
    Training,
    Labels,
    Eval,
}

/// Spec 026 v1 accepts only `maintainer-declared`. The value stays a string
/// so that a future amendment adding a status does not break sync.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ModelRightsVerification {
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ModelRightsChange {
    pub reason: String,
    pub evidence_url: String,
}

/// Index `model_usage[]` entry (FR-014).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PublicModelUsage {
    pub id: String,
    pub usage_class: ModelUsageClass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelUsageClass {
    Unrestricted,
    EvaluationOnly,
    Conditional,
}

/// Index entry `status` (FR-013/FR-014). Revoked is never active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicRecordStatus {
    Active,
    Deprecated,
    Revoked,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)]

    use super::*;
    use serde_json::json;

    fn full_model_ref() -> serde_json::Value {
        json!({
            "id": "example-org/tiny-ner",
            "huggingface_id": "example-org/tiny-ner",
            "revision": "0123456789abcdef0123456789abcdef01234567",
            "spdx_expression": "Apache-2.0",
            "attribution_required": true,
            "copyright": "Copyright 2026 The example-org/tiny-ner authors",
            "commercial_use": "allowed",
            "redistribution": "allowed",
            "derivatives": "allowed",
            "license_files": [{"url": "https://example.invalid/LICENSE", "sha256": "aa"}],
            "notice_files": [{"url": "https://example.invalid/NOTICE", "sha256": "bb"}],
            "derivation": {
                "transformations": ["quantize", "format-convert"],
                "description": "int8",
                "tool_url": "https://example.invalid/tool",
                "converted_sha256": "cc"
            },
            "data_obligations": [{
                "dataset": "corpus",
                "kind": "labels",
                "obligation": "attribute",
                "source_url": "https://example.invalid/corpus",
                "spdx_expression": "CC-BY-4.0"
            }],
            "verification": {"status": "maintainer-declared", "evidence_url": "https://example.invalid/card"},
            "rights_change": {"reason": "relicensed", "evidence_url": "https://example.invalid/why"}
        })
    }

    fn parse(value: serde_json::Value) -> PublicModelRef {
        match serde_json::from_value::<PublicModelReference>(value) {
            Ok(PublicModelReference::Object(model_ref)) => *model_ref,
            other => panic!("expected an object ModelRef, got {other:?}"),
        }
    }

    #[test]
    fn full_rights_record_round_trips_unchanged() {
        let value = full_model_ref();
        let model_ref = parse(value.clone());
        assert_eq!(serde_json::to_value(&model_ref).ok(), Some(value));
        assert_eq!(
            match &model_ref.derivation {
                Some(ModelDerivationDeclaration::Converted(derivation)) => {
                    Some(derivation.transformations.clone())
                }
                _ => None,
            },
            Some(vec![
                ModelTransformation::Quantize,
                ModelTransformation::FormatConvert
            ])
        );
        assert_eq!(
            model_ref.data_obligations.as_ref().map(|d| d[0].kind),
            Some(ModelDataKind::Labels)
        );
    }

    #[test]
    fn null_derivation_is_distinct_from_absent() {
        let mut value = full_model_ref();
        value["derivation"] = serde_json::Value::Null;
        let verbatim = parse(value.clone());
        assert_eq!(
            verbatim.derivation,
            Some(ModelDerivationDeclaration::Verbatim)
        );
        assert_eq!(
            serde_json::to_value(&verbatim)
                .ok()
                .map(|v| v["derivation"].is_null()),
            Some(true)
        );

        if let Some(object) = value.as_object_mut() {
            object.remove("derivation");
        }
        assert_eq!(parse(value).derivation, None);
    }

    #[test]
    fn legacy_and_pre_026_references_deserialize() {
        let ai: PublicAiDeclaration = serde_json::from_value(json!({
            "model_backed": true,
            "models": [
                "openai/whisper-tiny",
                {
                    "id": "snakers4/silero-vad",
                    "spdx_expression": "MIT",
                    "attribution_required": true,
                    "source_url": "https://github.com/snakers4/silero-vad/blob/debc3a0002a266fe690fda265a0685f5e0184bd9/x.onnx"
                }
            ]
        }))
        .unwrap_or_else(|error| panic!("legacy ai must deserialize: {error}"));
        assert_eq!(ai.models[0].id(), "openai/whisper-tiny");
        assert_eq!(ai.models[1].id(), "snakers4/silero-vad");
        let PublicModelReference::Object(pre_026) = &ai.models[1] else {
            panic!("expected object reference");
        };
        assert_eq!(pre_026.usage_class(), None);
        assert!(pre_026.data_obligations.is_none());
        assert!(pre_026.derivation.is_none());

        let plain: PublicAiDeclaration = serde_json::from_value(json!({"model_backed": false}))
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(plain.models.is_empty());
    }

    #[test]
    fn usage_class_is_derived_from_rights_enums() {
        let mut model_ref = parse(full_model_ref());
        assert_eq!(model_ref.usage_class(), Some(ModelUsageClass::Unrestricted));
        model_ref.commercial_use = Some(ModelRight::Forbidden);
        assert_eq!(
            model_ref.usage_class(),
            Some(ModelUsageClass::EvaluationOnly)
        );
        model_ref.commercial_use = Some(ModelRight::Allowed);
        model_ref.derivatives = Some(ModelRight::Conditional);
        assert_eq!(model_ref.usage_class(), Some(ModelUsageClass::Conditional));
        model_ref.derivatives = Some(ModelRight::Unknown);
        assert_eq!(model_ref.usage_class(), Some(ModelUsageClass::Conditional));
        model_ref.redistribution = None;
        assert_eq!(model_ref.usage_class(), None);
    }

    #[test]
    fn enum_wire_names_match_spec_026() {
        let names = serde_json::to_value((
            ModelUsageClass::EvaluationOnly,
            ModelTransformation::FineTune,
            PublicRecordStatus::Revoked,
            ModelDataKind::Eval,
            ModelRight::Conditional,
        ))
        .ok();
        assert_eq!(
            names,
            Some(json!([
                "evaluation-only",
                "fine-tune",
                "revoked",
                "eval",
                "conditional"
            ]))
        );
    }

    #[test]
    fn unknown_enum_value_is_rejected() {
        let mut value = full_model_ref();
        value["commercial_use"] = json!("yes");
        assert!(matches!(
            serde_json::from_value::<PublicModelReference>(value),
            Ok(PublicModelReference::Legacy(_)) | Err(_)
        ));
    }
}
