#![allow(clippy::expect_used)]

use serde_json::json;
use traverse_contracts::{
    EventClassification, EventContract, EventPayload, EventProvenance, EventProvenanceSource,
    EventType, IdReference, Lifecycle, Owner, PayloadCompatibility,
};
use traverse_registry::{
    DataClassification, EventProductDescriptor, EventProductErrorCode, EventProductReplacement,
    FieldClassification, validate_event_product_descriptor,
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
            classification: DataClassification::Internal,
        },
        FieldClassification {
            field_path: "author_email".to_string(),
            classification: DataClassification::Confidential,
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
        field_classifications: base_field_classifications(),
        replacement: None,
    }
}

fn error_codes(failure: &traverse_registry::EventProductValidationFailure) -> Vec<EventProductErrorCode> {
    failure.errors.iter().map(|error| error.code).collect()
}

#[test]
fn accepts_valid_event_product_descriptor() {
    let descriptor = base_descriptor();
    assert!(validate_event_product_descriptor(&descriptor, None).is_ok());
}

#[test]
fn rejects_missing_support_route() {
    let mut descriptor = base_descriptor();
    descriptor.support_route = String::new();

    let failure = validate_event_product_descriptor(&descriptor, None)
        .expect_err("empty support route should fail");

    assert!(error_codes(&failure).contains(&EventProductErrorCode::MissingSupportRoute));
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
    descriptor.field_classifications.retain(|entry| entry.field_path != "author_email");

    let failure = validate_event_product_descriptor(&descriptor, None)
        .expect_err("missing classification for a declared property should fail");

    assert!(error_codes(&failure).contains(&EventProductErrorCode::MissingFieldClassification));
}

#[test]
fn rejects_unexpected_field_classification() {
    let mut descriptor = base_descriptor();
    descriptor.field_classifications.push(FieldClassification {
        field_path: "not_a_declared_property".to_string(),
        classification: DataClassification::Public,
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

#[test]
fn accepts_irregular_past_tense_name() {
    let mut descriptor = base_descriptor();
    descriptor.contract.id = "content.comments.comment-draft-sent".to_string();
    descriptor.contract.name = "comment-draft-sent".to_string();

    assert!(validate_event_product_descriptor(&descriptor, None).is_ok());
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
