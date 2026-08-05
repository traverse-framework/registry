use traverse_contracts::{ErrorSeverity, EventContract, Lifecycle};

/// Governing spec for this module: `specs/016-ecca-event-product-adoption`.
pub const EVENT_PRODUCT_GOVERNING_SPEC: &str = "016-ecca-event-product-adoption";

/// ECCA-additive metadata layered on top of an already-validated [`EventContract`]
/// (governed by `specs/012-event-registry-adoption`). `traverse-contracts` is an
/// exact-pinned external dependency, so this descriptor cannot add fields to
/// `EventContract` itself -- it composes around it instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventProductDescriptor {
    pub contract: EventContract,
    pub support_route: String,
    pub field_classifications: Vec<FieldClassification>,
    pub replacement: Option<EventProductReplacement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldClassification {
    pub field_path: String,
    pub classification: DataClassification,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataClassification {
    Public,
    Internal,
    Confidential,
    Restricted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventProductReplacement {
    pub event_id: String,
    pub version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventProductErrorCode {
    MissingSupportRoute,
    InvalidSupportRoute,
    MissingFieldClassification,
    UnexpectedFieldClassification,
    DuplicateFieldClassification,
    MissingReplacement,
    UnexpectedReplacement,
    InvalidReplacement,
    NonPastTenseName,
    ImmutableDescriptorConflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventProductValidationError {
    pub code: EventProductErrorCode,
    pub path: String,
    pub message: String,
    pub severity: ErrorSeverity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventProductValidationFailure {
    pub errors: Vec<EventProductValidationError>,
}

/// A curated allow-list of irregular past-tense/past-participle domain-fact
/// endings that don't end in the regular `-ed` suffix (FR-004,
/// `016-ecca-event-product-adoption`). Deliberately small and closed rather
/// than a general English past-tense detector, to keep the check deterministic.
const IRREGULAR_PAST_TENSE_ENDINGS: &[&str] = &[
    "sent", "received", "paid", "shipped", "built", "sold", "bought", "held",
    "left", "spent", "lost", "won", "met", "set", "begun", "grown", "shown",
    "known", "seen", "done", "gone", "made", "given", "taken", "written",
    "broken", "chosen", "spoken", "frozen", "stolen", "torn", "worn", "found",
];

/// Validates one [`EventProductDescriptor`] against the ECCA event-product
/// rules (`specs/016-ecca-event-product-adoption`), on top of the
/// `specs/012-event-registry-adoption` contract validation already applied to
/// `descriptor.contract` by `EventRegistry::register`.
///
/// `existing` is the previously validated descriptor for the same `(scope,
/// id, version)`, when one is already published -- the caller (a future
/// registry-side store) is responsible for locating it; this function stays
/// storage-free and only enforces that its own content does not change once
/// published, mirroring how `EventValidationContext::existing_published`
/// keeps `validate_event_contract` storage-agnostic.
///
/// # Errors
///
/// Returns [`EventProductValidationFailure`] when the support route, field
/// classifications, lifecycle/replacement pairing, semantic naming, or
/// immutable republication rules are violated.
pub fn validate_event_product_descriptor(
    descriptor: &EventProductDescriptor,
    existing: Option<&EventProductDescriptor>,
) -> Result<(), EventProductValidationFailure> {
    let mut errors = Vec::new();

    validate_support_route(&descriptor.support_route, &mut errors);
    validate_field_classifications(
        &descriptor.contract,
        &descriptor.field_classifications,
        &mut errors,
    );
    validate_replacement(
        &descriptor.contract,
        descriptor.replacement.as_ref(),
        &mut errors,
    );
    validate_past_tense_name(&descriptor.contract.name, &mut errors);

    if let Some(existing) = existing
        && existing != descriptor
    {
        errors.push(event_product_error(
            EventProductErrorCode::ImmutableDescriptorConflict,
            "$",
            "a published event-product descriptor is immutable; register a new version instead",
        ));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(EventProductValidationFailure { errors })
    }
}

fn validate_support_route(support_route: &str, errors: &mut Vec<EventProductValidationError>) {
    if support_route.is_empty() {
        errors.push(event_product_error(
            EventProductErrorCode::MissingSupportRoute,
            "$.support_route",
            "support_route is required so consumers have a stable owner support path",
        ));
        return;
    }

    if !support_route.starts_with("https://") {
        errors.push(event_product_error(
            EventProductErrorCode::InvalidSupportRoute,
            "$.support_route",
            "support_route must be an https:// URL",
        ));
    }
}

fn declared_payload_properties(contract: &EventContract) -> Vec<String> {
    contract
        .payload
        .schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .map(|properties| properties.keys().cloned().collect())
        .unwrap_or_default()
}

fn validate_field_classifications(
    contract: &EventContract,
    field_classifications: &[FieldClassification],
    errors: &mut Vec<EventProductValidationError>,
) {
    let declared_properties = declared_payload_properties(contract);

    let mut seen = std::collections::BTreeSet::new();
    for entry in field_classifications {
        if !seen.insert(entry.field_path.clone()) {
            errors.push(event_product_error(
                EventProductErrorCode::DuplicateFieldClassification,
                "$.field_classifications",
                &format!("duplicate field classification for '{}'", entry.field_path),
            ));
        }

        if !declared_properties.contains(&entry.field_path) {
            errors.push(event_product_error(
                EventProductErrorCode::UnexpectedFieldClassification,
                "$.field_classifications",
                &format!(
                    "'{}' is not a declared top-level payload property",
                    entry.field_path
                ),
            ));
        }
    }

    for property in &declared_properties {
        if !field_classifications
            .iter()
            .any(|entry| &entry.field_path == property)
        {
            errors.push(event_product_error(
                EventProductErrorCode::MissingFieldClassification,
                "$.field_classifications",
                &format!(
                    "payload property '{property}' has no controlled-exposure classification"
                ),
            ));
        }
    }
}

fn validate_replacement(
    contract: &EventContract,
    replacement: Option<&EventProductReplacement>,
    errors: &mut Vec<EventProductValidationError>,
) {
    let requires_replacement = matches!(contract.lifecycle, Lifecycle::Deprecated | Lifecycle::Retired);
    let forbids_replacement = matches!(contract.lifecycle, Lifecycle::Draft | Lifecycle::Active);

    match replacement {
        None if requires_replacement => {
            errors.push(event_product_error(
                EventProductErrorCode::MissingReplacement,
                "$.replacement",
                "deprecated or retired events must declare a replacement event",
            ));
        }
        Some(_) if forbids_replacement => {
            errors.push(event_product_error(
                EventProductErrorCode::UnexpectedReplacement,
                "$.replacement",
                "draft or active events must not declare a replacement",
            ));
        }
        Some(replacement) => {
            if replacement.event_id.is_empty() || replacement.version.is_empty() {
                errors.push(event_product_error(
                    EventProductErrorCode::InvalidReplacement,
                    "$.replacement",
                    "replacement event_id and version are required when present",
                ));
            } else if replacement.event_id == contract.id && replacement.version == contract.version
            {
                errors.push(event_product_error(
                    EventProductErrorCode::InvalidReplacement,
                    "$.replacement",
                    "an event cannot declare itself as its own replacement",
                ));
            }
        }
        None => {}
    }
}

fn validate_past_tense_name(name: &str, errors: &mut Vec<EventProductValidationError>) {
    let Some(last_segment) = name.split('-').next_back().filter(|segment| !segment.is_empty())
    else {
        errors.push(event_product_error(
            EventProductErrorCode::NonPastTenseName,
            "$.name",
            "name must end with a past-tense domain-fact segment",
        ));
        return;
    };

    let is_past_tense =
        last_segment.ends_with("ed") || IRREGULAR_PAST_TENSE_ENDINGS.contains(&last_segment);

    if !is_past_tense {
        errors.push(event_product_error(
            EventProductErrorCode::NonPastTenseName,
            "$.name",
            &format!(
                "'{last_segment}' is not past tense; ECCA event names MUST describe a fact that already happened"
            ),
        ));
    }
}

fn event_product_error(
    code: EventProductErrorCode,
    path: &str,
    message: &str,
) -> EventProductValidationError {
    EventProductValidationError {
        code,
        path: path.to_string(),
        message: message.to_string(),
        severity: ErrorSeverity::Error,
    }
}
