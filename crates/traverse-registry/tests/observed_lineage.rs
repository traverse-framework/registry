#![allow(clippy::expect_used)]

use serde_json::json;
use traverse_contracts::{
    EventClassification, EventContract, EventPayload, EventProvenance, EventProvenanceSource,
    EventType, IdReference, Lifecycle, Owner, PayloadCompatibility,
};
use traverse_registry::{
    DataClassification, DriftKind, EventProductDescriptor, EventProductRegistration,
    EventProductRegistry, FieldClassification, ObservedEventInteraction, ObservedLineageStore,
    ObservedRole, RegistryScope,
};

fn descriptor(id: &str, name: &str, publisher: &str) -> EventProductDescriptor {
    EventProductDescriptor {
        contract: EventContract {
            kind: "event_contract".to_string(),
            schema_version: "1.0.0".to_string(),
            id: id.to_string(),
            namespace: "content.comments".to_string(),
            name: name.to_string(),
            version: "1.0.0".to_string(),
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
                    "properties": {"draft_id": {"type": "string"}}
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
        },
        support_route: "https://support.traverse.dev/comments".to_string(),
        field_classifications: vec![FieldClassification {
            field_path: "draft_id".to_string(),
            classification: DataClassification::Internal,
        }],
        replacement: None,
    }
}

fn interaction(event_id: &str, capability_id: &str, role: ObservedRole) -> ObservedEventInteraction {
    ObservedEventInteraction {
        event_id: event_id.to_string(),
        event_version: "1.0.0".to_string(),
        capability_id: capability_id.to_string(),
        role,
        observed_at: "2026-08-05T00:00:00Z".to_string(),
    }
}

#[test]
fn records_interaction_without_drift_when_capability_is_declared() {
    let mut store = ObservedLineageStore::new();
    let declared = vec!["content.comments.create-comment-draft".to_string()];

    store.record(
        interaction(
            "content.comments.comment-draft-created",
            "content.comments.create-comment-draft",
            ObservedRole::Publisher,
        ),
        &declared,
    );

    assert_eq!(
        store
            .interactions_for("content.comments.comment-draft-created", "1.0.0")
            .len(),
        1
    );
    assert!(
        store
            .drift_for("content.comments.comment-draft-created", "1.0.0")
            .is_empty()
    );
}

#[test]
fn records_drift_for_undeclared_publisher() {
    let mut store = ObservedLineageStore::new();
    let declared = vec!["content.comments.create-comment-draft".to_string()];

    store.record(
        interaction(
            "content.comments.comment-draft-created",
            "content.comments.rogue-capability",
            ObservedRole::Publisher,
        ),
        &declared,
    );

    let drift = store.drift_for("content.comments.comment-draft-created", "1.0.0");
    assert_eq!(drift.len(), 1);
    assert_eq!(drift[0].kind, DriftKind::UndeclaredPublisher);
    assert_eq!(drift[0].capability_id, "content.comments.rogue-capability");
}

#[test]
fn records_drift_for_undeclared_subscriber() {
    let mut store = ObservedLineageStore::new();
    let declared: Vec<String> = vec![];

    store.record(
        interaction(
            "content.comments.comment-draft-created",
            "content.comments.unexpected-subscriber",
            ObservedRole::Subscriber,
        ),
        &declared,
    );

    let drift = store.drift_for("content.comments.comment-draft-created", "1.0.0");
    assert_eq!(drift.len(), 1);
    assert_eq!(drift[0].kind, DriftKind::UndeclaredSubscriber);
}

#[test]
fn interactions_and_drift_are_scoped_to_the_requested_event_identity() {
    let mut store = ObservedLineageStore::new();
    store.record(
        interaction(
            "content.comments.comment-draft-created",
            "content.comments.rogue-capability",
            ObservedRole::Publisher,
        ),
        &[],
    );
    store.record(
        interaction(
            "content.comments.comment-draft-deleted",
            "content.comments.delete-comment-draft",
            ObservedRole::Publisher,
        ),
        &["content.comments.delete-comment-draft".to_string()],
    );

    assert_eq!(
        store
            .interactions_for("content.comments.comment-draft-created", "1.0.0")
            .len(),
        1
    );
    assert_eq!(
        store
            .interactions_for("content.comments.comment-draft-deleted", "1.0.0")
            .len(),
        1
    );
    assert!(
        store
            .drift_for("content.comments.comment-draft-deleted", "1.0.0")
            .is_empty()
    );
}

#[test]
fn recording_observations_never_affects_declared_registry_validation_outcomes() {
    let mut registry = EventProductRegistry::new();
    let request = EventProductRegistration {
        scope: RegistryScope::Public,
        descriptor: descriptor(
            "content.comments.comment-draft-created",
            "comment-draft-created",
            "content.comments.create-comment-draft",
        ),
    };
    registry.register(request).expect("registration should pass");

    let before = registry
        .find_exact(
            RegistryScope::Public,
            "content.comments.comment-draft-created",
            "1.0.0",
        )
        .cloned();

    let mut store = ObservedLineageStore::new();
    for _ in 0..5 {
        store.record(
            interaction(
                "content.comments.comment-draft-created",
                "content.comments.rogue-capability",
                ObservedRole::Publisher,
            ),
            &[],
        );
    }
    assert_eq!(
        store
            .drift_for("content.comments.comment-draft-created", "1.0.0")
            .len(),
        5
    );

    let after = registry
        .find_exact(
            RegistryScope::Public,
            "content.comments.comment-draft-created",
            "1.0.0",
        )
        .cloned();

    assert_eq!(before, after);

    let second = EventProductRegistration {
        scope: RegistryScope::Public,
        descriptor: descriptor(
            "content.comments.comment-draft-deleted",
            "comment-draft-deleted",
            "content.comments.delete-comment-draft",
        ),
    };
    registry
        .register(second)
        .expect("a second, unrelated registration should still pass identically");
}
