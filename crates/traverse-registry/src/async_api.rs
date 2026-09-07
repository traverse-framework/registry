//! `AsyncAPI` derived export for ECCA event products (spec
//! `016-ecca-event-product-adoption` FR-015, ADR-0028 point 2).
//!
//! `generate_async_api_document` is a pure function: it always regenerates
//! its output from an [`crate::EventProductDescriptor`] and never persists
//! or accepts a hand-authored document. There is no storage, no mutation
//! path, and no way to feed a hand-written `AsyncAPI` document back in --
//! the governed descriptor remains the only contract authority.

use crate::{DataClassification, EventExposureClass, EventProductDescriptor};
use serde_json::{Value, json};
use traverse_contracts::{CapabilityReference, Lifecycle};

const ASYNCAPI_VERSION: &str = "2.6.0";

/// Generates an `AsyncAPI` 2.6.0 document describing one event product.
///
/// The channel is keyed by the event's governed id and carries both a
/// `publish` and a `subscribe` operation using the same message -- from
/// the registry's perspective (not a single application's), the channel
/// is one thing that declared publishers write to and declared
/// subscribers read from. Everything ECCA-specific that `AsyncAPI` has no
/// native field for (support route, exposure, lifecycle, field
/// classifications, declared publisher/subscriber capability ids,
/// `CloudEvents` envelope mapping, delivery-semantics declarations) is
/// carried as `x-*` vendor extensions rather than dropped.
#[must_use]
pub fn generate_async_api_document(descriptor: &EventProductDescriptor) -> Value {
    let contract = &descriptor.contract;
    let message = message_object(descriptor);

    json!({
        "asyncapi": ASYNCAPI_VERSION,
        "info": {
            "title": contract.id,
            "version": contract.version,
            "description": contract.description,
        },
        "channels": {
            contract.id.clone(): {
                "description": contract.summary,
                "publish": { "message": message.clone() },
                "subscribe": { "message": message },
                "x-traverse-lifecycle": lifecycle_str(&contract.lifecycle),
                "x-traverse-exposure": exposure_str(descriptor.exposure),
                "x-traverse-support-route": descriptor.support_route,
                "x-traverse-publishers": capability_refs(&contract.publishers),
                "x-traverse-subscribers": capability_refs(&contract.subscribers),
                "x-traverse-field-classifications": field_classifications(descriptor),
                "x-traverse-cloud-events-source": descriptor.cloud_events_source,
                "x-traverse-cloud-events-subject-field": descriptor.cloud_events_subject_field,
                "x-traverse-deduplication-id-field": descriptor.deduplication_id_field,
                "x-traverse-ordering-scope-field": descriptor.ordering_scope_field,
                "x-traverse-correlation-id-field": descriptor.correlation_id_field,
                "x-traverse-causation-id-field": descriptor.causation_id_field,
                "x-traverse-retention-policy": descriptor.retention_policy,
            }
        },
    })
}

fn message_object(descriptor: &EventProductDescriptor) -> Value {
    let contract = &descriptor.contract;
    json!({
        "name": contract.name,
        "title": contract.name,
        "summary": contract.summary,
        "payload": contract.payload.schema,
    })
}

fn lifecycle_str(lifecycle: &Lifecycle) -> &'static str {
    match lifecycle {
        Lifecycle::Draft => "draft",
        Lifecycle::Active => "active",
        Lifecycle::Deprecated => "deprecated",
        Lifecycle::Retired => "retired",
        Lifecycle::Archived => "archived",
    }
}

fn exposure_str(exposure: EventExposureClass) -> &'static str {
    match exposure {
        EventExposureClass::Public => "public",
        EventExposureClass::Partner => "partner",
        EventExposureClass::Internal => "internal",
        EventExposureClass::Restricted => "restricted",
    }
}

fn capability_refs(references: &[CapabilityReference]) -> Value {
    Value::Array(
        references
            .iter()
            .map(|reference| {
                json!({
                    "capability_id": reference.capability_id,
                    "version": reference.version,
                })
            })
            .collect(),
    )
}

fn classification_str(classification: DataClassification) -> &'static str {
    match classification {
        DataClassification::NoClassification => "none",
        DataClassification::Personal => "personal",
        DataClassification::Sensitive => "sensitive",
        DataClassification::Regulated => "regulated",
    }
}

fn field_classifications(descriptor: &EventProductDescriptor) -> Value {
    Value::Array(
        descriptor
            .field_classifications
            .iter()
            .map(|entry| {
                json!({
                    "field_path": entry.field_path,
                    "classification": classification_str(entry.classification),
                })
            })
            .collect(),
    )
}
