#![allow(clippy::expect_used)]

use serde_json::json;
use traverse_contracts::{
    EventClassification, EventContract, EventPayload, EventProvenance, EventProvenanceSource,
    EventType, IdReference, Lifecycle, Owner, PayloadCompatibility,
};
use traverse_registry::{
    DataClassification, EventProductDescriptor, EventProductRegistration, EventProductRegistry,
    FieldClassification, LookupScope, RegistryScope,
};

fn event_contract(
    id: &str,
    name: &str,
    version: &str,
    publisher: &str,
    subscriber: Option<&str>,
) -> EventContract {
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
                    "draft_id": {"type": "string"}
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
            capability_id: publisher.to_string(),
            version: "1.0.0".to_string(),
        }],
        subscribers: subscriber
            .map(|capability_id| {
                vec![traverse_contracts::CapabilityReference {
                    capability_id: capability_id.to_string(),
                    version: "1.0.0".to_string(),
                }]
            })
            .unwrap_or_default(),
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

fn descriptor(
    id: &str,
    name: &str,
    version: &str,
    publisher: &str,
    subscriber: Option<&str>,
) -> EventProductDescriptor {
    EventProductDescriptor {
        contract: event_contract(id, name, version, publisher, subscriber),
        support_route: "https://support.traverse.dev/comments".to_string(),
        field_classifications: vec![FieldClassification {
            field_path: "draft_id".to_string(),
            classification: DataClassification::Internal,
        }],
        replacement: None,
    }
}

#[test]
fn registers_and_finds_a_descriptor_exactly() {
    let mut registry = EventProductRegistry::new();
    let request = EventProductRegistration {
        scope: RegistryScope::Public,
        descriptor: descriptor(
            "content.comments.comment-draft-created",
            "comment-draft-created",
            "1.0.0",
            "content.comments.create-comment-draft",
            None,
        ),
    };

    registry.register(request).expect("registration should pass");

    let found = registry
        .find_exact(
            RegistryScope::Public,
            "content.comments.comment-draft-created",
            "1.0.0",
        )
        .expect("descriptor should be found");
    assert_eq!(found.contract.id, "content.comments.comment-draft-created");
}

#[test]
fn rejects_invalid_descriptor_and_indexes_nothing() {
    let mut registry = EventProductRegistry::new();
    let mut invalid = descriptor(
        "content.comments.comment-draft-created",
        "comment-draft-created",
        "1.0.0",
        "content.comments.create-comment-draft",
        None,
    );
    invalid.support_route = String::new();

    let request = EventProductRegistration {
        scope: RegistryScope::Public,
        descriptor: invalid,
    };

    registry.register(request).expect_err("invalid descriptor should fail");

    assert!(
        registry
            .find_exact(
                RegistryScope::Public,
                "content.comments.comment-draft-created",
                "1.0.0",
            )
            .is_none()
    );
    assert!(registry
        .declared_publishes("content.comments.create-comment-draft")
        .is_empty());
}

#[test]
fn reregistering_identical_content_is_idempotent() {
    let mut registry = EventProductRegistry::new();
    let build = || EventProductRegistration {
        scope: RegistryScope::Public,
        descriptor: descriptor(
            "content.comments.comment-draft-created",
            "comment-draft-created",
            "1.0.0",
            "content.comments.create-comment-draft",
            None,
        ),
    };

    registry.register(build()).expect("first registration should pass");
    registry
        .register(build())
        .expect("identical re-registration should be idempotent");

    assert_eq!(
        registry
            .declared_publishes("content.comments.create-comment-draft")
            .len(),
        1
    );
}

#[test]
fn rejects_changed_content_for_the_same_identity_and_version() {
    let mut registry = EventProductRegistry::new();
    registry
        .register(EventProductRegistration {
            scope: RegistryScope::Public,
            descriptor: descriptor(
                "content.comments.comment-draft-created",
                "comment-draft-created",
                "1.0.0",
                "content.comments.create-comment-draft",
                None,
            ),
        })
        .expect("first registration should pass");

    let mut changed = descriptor(
        "content.comments.comment-draft-created",
        "comment-draft-created",
        "1.0.0",
        "content.comments.create-comment-draft",
        None,
    );
    changed.support_route = "https://support.traverse.dev/comments-v2".to_string();

    let failure = registry
        .register(EventProductRegistration {
            scope: RegistryScope::Public,
            descriptor: changed,
        })
        .expect_err("changed content for the same identity/version should fail");

    assert_eq!(failure.errors.len(), 1);
}

#[test]
fn private_scope_takes_precedence_over_public_on_discover() {
    let mut registry = EventProductRegistry::new();
    registry
        .register(EventProductRegistration {
            scope: RegistryScope::Public,
            descriptor: descriptor(
                "content.comments.comment-draft-created",
                "comment-draft-created",
                "1.0.0",
                "content.comments.create-comment-draft",
                None,
            ),
        })
        .expect("public registration should pass");
    registry
        .register(EventProductRegistration {
            scope: RegistryScope::Private,
            descriptor: descriptor(
                "content.comments.comment-draft-created",
                "comment-draft-created",
                "1.0.0",
                "content.comments.create-comment-draft-private",
                None,
            ),
        })
        .expect("private registration should pass");

    let results = registry.discover(LookupScope::PreferPrivate);
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].contract.publishers[0].capability_id,
        "content.comments.create-comment-draft-private"
    );

    let public_only = registry.discover(LookupScope::PublicOnly);
    assert_eq!(public_only.len(), 1);
    assert_eq!(
        public_only[0].contract.publishers[0].capability_id,
        "content.comments.create-comment-draft"
    );
}

#[test]
fn declared_publishes_and_consumes_index_by_capability() {
    let mut registry = EventProductRegistry::new();
    registry
        .register(EventProductRegistration {
            scope: RegistryScope::Public,
            descriptor: descriptor(
                "content.comments.comment-draft-created",
                "comment-draft-created",
                "1.0.0",
                "content.comments.create-comment-draft",
                Some("content.comments.publish-comment"),
            ),
        })
        .expect("registration should pass");

    let publishes = registry.declared_publishes("content.comments.create-comment-draft");
    assert_eq!(publishes.len(), 1);
    assert_eq!(publishes[0].contract.id, "content.comments.comment-draft-created");

    let consumes = registry.declared_consumes("content.comments.publish-comment");
    assert_eq!(consumes.len(), 1);
    assert_eq!(consumes[0].contract.id, "content.comments.comment-draft-created");

    assert!(registry.declared_publishes("unknown.capability").is_empty());
    assert!(registry.declared_consumes("unknown.capability").is_empty());
}
