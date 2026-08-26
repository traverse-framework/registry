#![allow(clippy::expect_used)]

use serde_json::json;
use traverse_contracts::{
    EventClassification, EventContract, EventPayload, EventProvenance, EventProvenanceSource,
    EventType, IdReference, Lifecycle, Owner, PayloadCompatibility,
};
use traverse_registry::{
    DataClassification, EventExposureClass, EventProductDescriptor, EventProductErrorCode,
    EventProductReplacement, FieldClassification, validate_event_product_descriptor,
};

fn base_event_contract(id: &str, name: &str, version: &str) -> EventContract {
    EventContract {
        kind: "event_contract".to_string(),
        schema_version: "1.0.0".to_string(),
        id: id.to_string(),
        namespace: "content.comments".to_string(),
        name: name.to_string(),
        version: version.to_string(),
        lifecycle: Lifecycle::Active,
        owner: Owner {
            team: "traverse-core".to_string(),
            contact: "enrico.piovesan10@gmail.com".to_string(),
        },
        summary: "Published when a comment draft has been created.".to_string(),
        description: "Governed event contract for comment draft creation.".to_string(),
        payload: EventPayload {
            schema: json!({
                "type": "object",
                "required": ["draft_id"],
                "properties": {
                    "draft_id": {"type": "string"},
                    "author_email": {"type": "string"}
                }
            }),
            compatibility: PayloadCompatibility::BackwardCompatible,
        },
        classification: EventClassification {
            domain: "content.comments".to_string(),
            bounded_context: "comments".to_string(),
            event_type: EventType::Domain,
            tags: vec!["comments".to_string()],
        },
        publishers: vec![traverse_contracts::CapabilityReference {
            capability_id: "content.comments.create-comment-draft".to_string(),
            version: "1.0.0".to_string(),
        }],
        subscribers: vec![],
        policies: vec![IdReference {
            id: "default-comment-safety".to_string(),
        }],
        tags: vec!["comments".to_string()],
        provenance: EventProvenance {
            source: EventProvenanceSource::Greenfield,
            author: "enricopiovesan".to_string(),
            created_at: "2026-03-30T00:00:00Z".to_string(),
        },
        evidence: vec![],
    }
}

fn base_field_classifications() -> Vec<FieldClassification> {
    vec![
        FieldClassification {
            field_path: "draft_id".to_string(),
            classification: DataClassification::NoClassification,
        },
        FieldClassification {
            field_path: "author_email".to_string(),
            classification: DataClassification::Personal,
        },
    ]
}

fn base_descriptor() -> EventProductDescriptor {
    EventProductDescriptor {
        contract: base_event_contract(
            "content.comments.comment-draft-created",
            "comment-draft-created",
            "1.0.0",
        ),
        support_route: "https://support.traverse.dev/comments".to_string(),
        exposure: EventExposureClass::Internal,
        field_classifications: base_field_classifications(),
        replacement: None,
        cloud_events_source: "traverse://capability/content.comments.create-comment-draft"
            .to_string(),
        cloud_events_subject_field: Some("draft_id".to_string()),
        deduplication_id_field: "draft_id".to_string(),
        ordering_scope_field: None,
        correlation_id_field: "envelope.correlation_id".to_string(),
        causation_id_field: None,
        retention_policy: "retain 90 days".to_string(),
    }
}

fn error_codes(
    failure: &traverse_registry::EventProductValidationFailure,
) -> Vec<EventProductErrorCode> {
    failure.errors.iter().map(|error| error.code).collect()
}

#[test]
fn accepts_valid_event_product_descriptor() {
    let descriptor = base_descriptor();
    assert!(validate_event_product_descriptor(&descriptor, None).is_ok());
}

#[test]
fn every_exposure_class_is_constructible() {
    for exposure in [
        EventExposureClass::Public,
        EventExposureClass::Partner,
        EventExposureClass::Internal,
        EventExposureClass::Restricted,
    ] {
        let mut descriptor = base_descriptor();
        descriptor.exposure = exposure;
        assert!(validate_event_product_descriptor(&descriptor, None).is_ok());
    }
}

#[test]
fn rejects_missing_support_route() {
    let mut descriptor = base_descriptor();
    descriptor.support_route = String::new();

    let failure = validate_event_product_descriptor(&descriptor, None)
        .expect_err("empty support route should fail");

    assert!(error_codes(&failure).contains(&EventProductErrorCode::MissingSupportRoute));
    let error = &failure.errors[0];
    assert_eq!(error.contract_id, descriptor.contract.id);
    assert_eq!(error.contract_version, descriptor.contract.version);
    assert!(!error.remediation.is_empty());
    assert_eq!(error.governing_spec, "016-ecca-event-product-adoption");
}

#[test]
fn rejects_invalid_support_route_scheme() {
    let mut descriptor = base_descriptor();
    descriptor.support_route = "http://support.traverse.dev/comments".to_string();

    let failure = validate_event_product_descriptor(&descriptor, None)
        .expect_err("non-https support route should fail");

    assert!(error_codes(&failure).contains(&EventProductErrorCode::InvalidSupportRoute));
}

#[test]
fn rejects_missing_field_classification() {
    let mut descriptor = base_descriptor();
    descriptor
        .field_classifications
        .retain(|entry| entry.field_path != "author_email");

    let failure = validate_event_product_descriptor(&descriptor, None)
        .expect_err("missing classification for a declared property should fail");

    assert!(error_codes(&failure).contains(&EventProductErrorCode::MissingFieldClassification));
}

#[test]
fn rejects_unexpected_field_classification() {
    let mut descriptor = base_descriptor();
    descriptor.field_classifications.push(FieldClassification {
        field_path: "not_a_declared_property".to_string(),
        classification: DataClassification::NoClassification,
    });

    let failure = validate_event_product_descriptor(&descriptor, None)
        .expect_err("classification for an undeclared property should fail");

    assert!(error_codes(&failure).contains(&EventProductErrorCode::UnexpectedFieldClassification));
}

#[test]
fn rejects_duplicate_field_classification() {
    let mut descriptor = base_descriptor();
    let duplicate = descriptor.field_classifications[0].classification;
    descriptor.field_classifications.push(FieldClassification {
        field_path: descriptor.field_classifications[0].field_path.clone(),
        classification: duplicate,
    });

    let failure = validate_event_product_descriptor(&descriptor, None)
        .expect_err("duplicate classification for the same field should fail");

    assert!(error_codes(&failure).contains(&EventProductErrorCode::DuplicateFieldClassification));
}

#[test]
fn every_field_classification_value_is_constructible() {
    for classification in [
        DataClassification::NoClassification,
        DataClassification::Personal,
        DataClassification::Sensitive,
        DataClassification::Regulated,
    ] {
        let mut descriptor = base_descriptor();
        descriptor.field_classifications = vec![
            FieldClassification {
                field_path: "draft_id".to_string(),
                classification,
            },
            FieldClassification {
                field_path: "author_email".to_string(),
                classification: DataClassification::Personal,
            },
        ];
        assert!(validate_event_product_descriptor(&descriptor, None).is_ok());
    }
}

#[test]
fn rejects_deprecated_event_without_replacement() {
    let mut descriptor = base_descriptor();
    descriptor.contract.lifecycle = Lifecycle::Deprecated;

    let failure = validate_event_product_descriptor(&descriptor, None)
        .expect_err("deprecated event without a replacement should fail");

    assert!(error_codes(&failure).contains(&EventProductErrorCode::MissingReplacement));
}

#[test]
fn accepts_deprecated_event_with_replacement() {
    let mut descriptor = base_descriptor();
    descriptor.contract.lifecycle = Lifecycle::Deprecated;
    descriptor.replacement = Some(EventProductReplacement {
        event_id: "content.comments.comment-draft-created-v2".to_string(),
        version: "1.0.0".to_string(),
    });

    assert!(validate_event_product_descriptor(&descriptor, None).is_ok());
}

#[test]
fn rejects_active_event_with_replacement() {
    let mut descriptor = base_descriptor();
    descriptor.replacement = Some(EventProductReplacement {
        event_id: "content.comments.comment-draft-created-v2".to_string(),
        version: "1.0.0".to_string(),
    });

    let failure = validate_event_product_descriptor(&descriptor, None)
        .expect_err("an active event must not declare a replacement");

    assert!(error_codes(&failure).contains(&EventProductErrorCode::UnexpectedReplacement));
}

#[test]
fn rejects_self_referential_replacement() {
    let mut descriptor = base_descriptor();
    descriptor.contract.lifecycle = Lifecycle::Retired;
    descriptor.replacement = Some(EventProductReplacement {
        event_id: descriptor.contract.id.clone(),
        version: descriptor.contract.version.clone(),
    });

    let failure = validate_event_product_descriptor(&descriptor, None)
        .expect_err("an event cannot replace itself");

    assert!(error_codes(&failure).contains(&EventProductErrorCode::InvalidReplacement));
}

/// registry#324: `id` must equal `"<namespace>.<name>"` exactly -- the
/// invariant `traverse-contracts`' own `EventRegistry::register` enforces
/// at load time, which this repo's own publish path never checked before
/// (a real bad publish, `core.action-item.status-transitioned`, slipped
/// through with `namespace="core"`, `name="status-transitioned"`, but
/// `id="core.action-item.status-transitioned"`).
#[test]
fn rejects_inconsistent_event_identity() {
    let mut descriptor = base_descriptor();
    descriptor.contract.id = "content.comments.extra-segment.comment-draft-created".to_string();

    let failure = validate_event_product_descriptor(&descriptor, None)
        .expect_err("id not equal to namespace.name should fail");

    assert!(error_codes(&failure).contains(&EventProductErrorCode::InconsistentIdentity));
    let error = failure
        .errors
        .iter()
        .find(|error| error.code == EventProductErrorCode::InconsistentIdentity)
        .expect("InconsistentIdentity error should be present");
    assert_eq!(error.path, "$.id");
    assert!(!error.remediation.is_empty());
}

#[test]
fn rejects_non_past_tense_name() {
    let mut descriptor = base_descriptor();
    descriptor.contract.id = "content.comments.create-comment-draft-event".to_string();
    descriptor.contract.namespace = "content.comments".to_string();
    descriptor.contract.name = "create-comment-draft-event".to_string();

    let failure = validate_event_product_descriptor(&descriptor, None)
        .expect_err("present-tense/imperative naming should fail");

    assert!(error_codes(&failure).contains(&EventProductErrorCode::NonPastTenseName));
}

/// Spec 016 v2.0.0, FR-007: matches Traverse's own runtime `is_fact_type`
/// exactly -- whole-name `-ed` suffix, no irregular-verb allowance. v1.0.0
/// allowed this via a curated allow-list Traverse's own runtime doesn't
/// have; v2.0.0 deliberately narrows to stay consistent with the runtime
/// side actually enforcing it (NFR-006).
#[test]
fn rejects_irregular_past_participle_not_ending_in_ed() {
    let mut descriptor = base_descriptor();
    descriptor.contract.id = "content.comments.comment-draft-sent".to_string();
    descriptor.contract.name = "comment-draft-sent".to_string();

    let failure = validate_event_product_descriptor(&descriptor, None).expect_err(
        "'sent' does not end in '-ed' and must be rejected, matching Traverse's own runtime check",
    );

    assert!(error_codes(&failure).contains(&EventProductErrorCode::NonPastTenseName));
}

#[test]
fn rejects_missing_cloud_events_source() {
    let mut descriptor = base_descriptor();
    descriptor.cloud_events_source = String::new();

    let failure = validate_event_product_descriptor(&descriptor, None)
        .expect_err("empty cloud_events_source should fail");

    assert!(error_codes(&failure).contains(&EventProductErrorCode::MissingCloudEventsSource));
}

#[test]
fn rejects_cloud_events_subject_field_not_a_declared_property() {
    let mut descriptor = base_descriptor();
    descriptor.cloud_events_subject_field = Some("not_a_declared_property".to_string());

    let failure = validate_event_product_descriptor(&descriptor, None)
        .expect_err("subject field must be a declared payload property");

    assert!(error_codes(&failure).contains(&EventProductErrorCode::InvalidCloudEventsSubjectField));
}

#[test]
fn accepts_absent_cloud_events_subject_field() {
    let mut descriptor = base_descriptor();
    descriptor.cloud_events_subject_field = None;

    assert!(validate_event_product_descriptor(&descriptor, None).is_ok());
}

#[test]
fn rejects_missing_deduplication_id_field() {
    let mut descriptor = base_descriptor();
    descriptor.deduplication_id_field = String::new();

    let failure = validate_event_product_descriptor(&descriptor, None)
        .expect_err("empty deduplication_id_field should fail");

    assert!(error_codes(&failure).contains(&EventProductErrorCode::MissingDeduplicationIdField));
}

#[test]
fn rejects_missing_correlation_id_field() {
    let mut descriptor = base_descriptor();
    descriptor.correlation_id_field = String::new();

    let failure = validate_event_product_descriptor(&descriptor, None)
        .expect_err("empty correlation_id_field should fail");

    assert!(error_codes(&failure).contains(&EventProductErrorCode::MissingCorrelationIdField));
}

#[test]
fn accepts_absent_ordering_scope_and_causation_id() {
    let mut descriptor = base_descriptor();
    descriptor.ordering_scope_field = None;
    descriptor.causation_id_field = None;

    assert!(validate_event_product_descriptor(&descriptor, None).is_ok());
}

#[test]
fn rejects_missing_retention_policy() {
    let mut descriptor = base_descriptor();
    descriptor.retention_policy = String::new();

    let failure = validate_event_product_descriptor(&descriptor, None)
        .expect_err("empty retention_policy should fail");

    assert!(error_codes(&failure).contains(&EventProductErrorCode::MissingRetentionPolicy));
}

#[test]
fn accepts_identical_redeclaration_as_immutable_no_conflict() {
    let existing = base_descriptor();
    let candidate = base_descriptor();

    assert!(validate_event_product_descriptor(&candidate, Some(&existing)).is_ok());
}

#[test]
fn rejects_immutable_descriptor_conflict_when_content_changes() {
    let existing = base_descriptor();
    let mut candidate = base_descriptor();
    candidate.support_route = "https://support.traverse.dev/comments-v2".to_string();

    let failure = validate_event_product_descriptor(&candidate, Some(&existing))
        .expect_err("changing a published descriptor's content should fail");

    assert!(error_codes(&failure).contains(&EventProductErrorCode::ImmutableDescriptorConflict));
}
