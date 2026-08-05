#![allow(clippy::expect_used)]

use serde_json::json;
use traverse_contracts::{
    CapabilityReference, EventClassification, EventContract, EventPayload, EventProvenance,
    EventProvenanceSource, EventType, IdReference, Lifecycle, Owner, PayloadCompatibility,
};
use traverse_registry::{
    DataClassification, EventProductDescriptor, EventProductReplacement, FieldClassification,
    generate_async_api_document,
};

fn descriptor() -> EventProductDescriptor {
    EventProductDescriptor {
        contract: EventContract {
            kind: "event_contract".to_string(),
            schema_version: "1.0.0".to_string(),
            id: "content.comments.comment-draft-created".to_string(),
            namespace: "content.comments".to_string(),
            name: "comment-draft-created".to_string(),
            version: "1.0.0".to_string(),
            lifecycle: Lifecycle::Deprecated,
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
            publishers: vec![CapabilityReference {
                capability_id: "content.comments.create-comment-draft".to_string(),
                version: "1.0.0".to_string(),
            }],
            subscribers: vec![CapabilityReference {
                capability_id: "content.comments.publish-comment".to_string(),
                version: "2.1.0".to_string(),
            }],
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
        },
        support_route: "https://support.traverse.dev/comments".to_string(),
        field_classifications: vec![
            FieldClassification {
                field_path: "draft_id".to_string(),
                classification: DataClassification::Internal,
            },
            FieldClassification {
                field_path: "author_email".to_string(),
                classification: DataClassification::Confidential,
            },
        ],
        replacement: Some(EventProductReplacement {
            event_id: "content.comments.comment-draft-created-v2".to_string(),
            version: "1.0.0".to_string(),
        }),
    }
}

#[test]
fn top_level_asyncapi_fields_match_the_source_descriptor() {
    let document = generate_async_api_document(&descriptor());

    assert_eq!(document["asyncapi"], "2.6.0");
    assert_eq!(document["info"]["title"], "content.comments.comment-draft-created");
    assert_eq!(document["info"]["version"], "1.0.0");
    assert_eq!(
        document["info"]["description"],
        "Governed event contract for comment draft creation."
    );
}

#[test]
fn payload_schema_round_trips_exactly() {
    let source = descriptor();
    let document = generate_async_api_document(&source);

    let channel = &document["channels"]["content.comments.comment-draft-created"];
    assert_eq!(
        channel["publish"]["message"]["payload"],
        source.contract.payload.schema
    );
    assert_eq!(
        channel["subscribe"]["message"]["payload"],
        source.contract.payload.schema
    );
}

#[test]
fn declared_publishers_and_subscribers_round_trip_exactly() {
    let source = descriptor();
    let document = generate_async_api_document(&source);
    let channel = &document["channels"]["content.comments.comment-draft-created"];

    assert_eq!(
        channel["x-traverse-publishers"],
        json!([{"capability_id": "content.comments.create-comment-draft", "version": "1.0.0"}])
    );
    assert_eq!(
        channel["x-traverse-subscribers"],
        json!([{"capability_id": "content.comments.publish-comment", "version": "2.1.0"}])
    );
}

#[test]
fn ecca_additive_fields_round_trip_exactly() {
    let source = descriptor();
    let document = generate_async_api_document(&source);
    let channel = &document["channels"]["content.comments.comment-draft-created"];

    assert_eq!(channel["x-traverse-lifecycle"], "deprecated");
    assert_eq!(
        channel["x-traverse-support-route"],
        "https://support.traverse.dev/comments"
    );

    let classifications = channel["x-traverse-field-classifications"]
        .as_array()
        .expect("field classifications should be an array");
    assert_eq!(classifications.len(), 2);
    assert!(classifications.contains(&json!({
        "field_path": "draft_id",
        "classification": "internal"
    })));
    assert!(classifications.contains(&json!({
        "field_path": "author_email",
        "classification": "confidential"
    })));
}

#[test]
fn generation_is_deterministic_for_identical_input() {
    let source = descriptor();
    assert_eq!(
        generate_async_api_document(&source),
        generate_async_api_document(&source)
    );
}
