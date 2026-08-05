//! `AsyncAPI` derived export for ECCA event products (spec
//! `016-ecca-event-product-adoption` FR-011, ADR-0028 point 2).
//!
//! `generate_async_api_document` is a pure function: it always regenerates
//! its output from an [`crate::EventProductDescriptor`] and never persists
//! or accepts a hand-authored document. There is no storage, no mutation
//! path, and no way to feed a hand-written `AsyncAPI` document back in --
//! the governed descriptor remains the only contract authority.

use crate::{DataClassification, EventProductDescriptor};
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
/// native field for (support route, lifecycle, field classifications,
/// declared publisher/subscriber capability ids) is carried as `x-*`
/// vendor extensions rather than dropped.
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
                "x-traverse-support-route": descriptor.support_route,
                "x-traverse-publishers": capability_refs(&contract.publishers),
                "x-traverse-subscribers": capability_refs(&contract.subscribers),
                "x-traverse-field-classifications": field_classifications(descriptor),
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
        DataClassification::Public => "public",
        DataClassification::Internal => "internal",
        DataClassification::Confidential => "confidential",
        DataClassification::Restricted => "restricted",
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
