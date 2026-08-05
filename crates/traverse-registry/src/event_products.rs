use crate::{LookupScope, RegistryScope};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use traverse_contracts::{ErrorSeverity, EventContract, EventType, Lifecycle};

/// Governing spec for this module: `specs/016-ecca-event-product-adoption` (v2.0.0).
pub const EVENT_PRODUCT_GOVERNING_SPEC: &str = "016-ecca-event-product-adoption";

/// ECCA-additive metadata layered on top of an already-validated [`EventContract`]
/// (governed by `specs/012-event-registry-adoption`). `traverse-contracts` is an
/// exact-pinned external dependency, so this descriptor cannot add fields to
/// `EventContract` itself -- it composes around it instead.
///
/// `Serialize`/`Deserialize` exist so this descriptor can be expressed as a
/// portable JSON conformance fixture (`crates/traverse-registry/fixtures/`)
/// for cross-repo consumers -- not because this crate persists it to disk
/// itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventProductDescriptor {
    pub contract: EventContract,
    pub support_route: String,
    /// Exactly one exposure class (Spec 534 FR-007). A Rust enum, not a
    /// free-form string, so "exactly one" and "free-form class names are
    /// invalid" both hold by construction.
    pub exposure: EventExposureClass,
    pub field_classifications: Vec<FieldClassification>,
    pub replacement: Option<EventProductReplacement>,
    /// CloudEvents-compatible `source` mapping (Spec 534 FR-005). `type` is
    /// already satisfied by the underlying, already-validated
    /// `EventContract.id`; `time` is a runtime-populated envelope value, out
    /// of scope for a registry-side descriptor.
    pub cloud_events_source: String,
    /// Which declared payload property maps to the `CloudEvents` `subject`,
    /// if any.
    pub cloud_events_subject_field: Option<String>,
    /// Which field or envelope concept serves as the deduplication identity
    /// (Spec 534 FR-010). Declaration only -- enforcement stays in Traverse.
    pub deduplication_id_field: String,
    /// Which field or envelope concept serves as the ordering scope, if
    /// this event has one (Spec 534 FR-010).
    pub ordering_scope_field: Option<String>,
    /// Which field or envelope concept serves as the correlation identity
    /// (Spec 534 FR-010).
    pub correlation_id_field: String,
    /// Which field or envelope concept serves as the causation identity, if
    /// this event has one (Spec 534 FR-010).
    pub causation_id_field: Option<String>,
    /// Non-empty retention policy statement (Spec 534 FR-005).
    pub retention_policy: String,
}

/// Top-level exposure class (Spec 534 FR-007) -- separate from per-field
/// [`DataClassification`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventExposureClass {
    Public,
    Partner,
    Internal,
    Restricted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldClassification {
    pub field_path: String,
    pub classification: DataClassification,
}

/// Per-field controlled data classification, using Spec 534 FR-007's exact
/// vocabulary (`none`/`personal`/`sensitive`/`regulated` on the wire).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataClassification {
    #[serde(rename = "none")]
    NoClassification,
    Personal,
    Sensitive,
    Regulated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    MissingCloudEventsSource,
    InvalidCloudEventsSubjectField,
    MissingDeduplicationIdField,
    MissingCorrelationIdField,
    MissingRetentionPolicy,
}

/// Matches Traverse's own `EventValidationDiagnostic` shape
/// (`crates/traverse-runtime/src/events/validation.rs`) and Spec 534
/// FR-012: every diagnostic carries enough to act on without cross-referencing
/// anything else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventProductValidationError {
    pub code: EventProductErrorCode,
    pub path: String,
    pub message: String,
    pub severity: ErrorSeverity,
    pub remediation: String,
    pub contract_id: String,
    pub contract_version: String,
    pub governing_spec: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventProductValidationFailure {
    pub errors: Vec<EventProductValidationError>,
}

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
/// Returns [`EventProductValidationFailure`] when any ECCA-additive field,
/// the semantic naming rule, or the immutable republication rule is
/// violated.
pub fn validate_event_product_descriptor(
    descriptor: &EventProductDescriptor,
    existing: Option<&EventProductDescriptor>,
) -> Result<(), EventProductValidationFailure> {
    let contract = &descriptor.contract;
    let mut errors = Vec::new();

    validate_support_route(contract, &descriptor.support_route, &mut errors);
    validate_field_classifications(contract, &descriptor.field_classifications, &mut errors);
    validate_replacement(contract, descriptor.replacement.as_ref(), &mut errors);
    validate_past_tense_name(contract, &mut errors);
    validate_cloud_events_mapping(
        contract,
        &descriptor.cloud_events_source,
        descriptor.cloud_events_subject_field.as_deref(),
        &mut errors,
    );
    validate_delivery_semantics(
        contract,
        &descriptor.deduplication_id_field,
        &descriptor.correlation_id_field,
        &mut errors,
    );
    validate_retention_policy(contract, &descriptor.retention_policy, &mut errors);

    if let Some(existing) = existing
        && existing != descriptor
    {
        errors.push(event_product_error(
            EventProductErrorCode::ImmutableDescriptorConflict,
            "$",
            "a published event-product descriptor is immutable; register a new version instead",
            contract,
        ));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(EventProductValidationFailure { errors })
    }
}

fn validate_support_route(
    contract: &EventContract,
    support_route: &str,
    errors: &mut Vec<EventProductValidationError>,
) {
    if support_route.is_empty() {
        errors.push(event_product_error(
            EventProductErrorCode::MissingSupportRoute,
            "$.support_route",
            "support_route is required so consumers have a stable owner support path",
            contract,
        ));
        return;
    }

    if !support_route.starts_with("https://") {
        errors.push(event_product_error(
            EventProductErrorCode::InvalidSupportRoute,
            "$.support_route",
            "support_route must be an https:// URL",
            contract,
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

    let mut seen = BTreeSet::new();
    for entry in field_classifications {
        if !seen.insert(entry.field_path.clone()) {
            errors.push(event_product_error(
                EventProductErrorCode::DuplicateFieldClassification,
                "$.field_classifications",
                &format!("duplicate field classification for '{}'", entry.field_path),
                contract,
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
                contract,
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
                contract,
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
                contract,
            ));
        }
        Some(_) if forbids_replacement => {
            errors.push(event_product_error(
                EventProductErrorCode::UnexpectedReplacement,
                "$.replacement",
                "draft or active events must not declare a replacement",
                contract,
            ));
        }
        Some(replacement) => {
            if replacement.event_id.is_empty() || replacement.version.is_empty() {
                errors.push(event_product_error(
                    EventProductErrorCode::InvalidReplacement,
                    "$.replacement",
                    "replacement event_id and version are required when present",
                    contract,
                ));
            } else if replacement.event_id == contract.id && replacement.version == contract.version
            {
                errors.push(event_product_error(
                    EventProductErrorCode::InvalidReplacement,
                    "$.replacement",
                    "an event cannot declare itself as its own replacement",
                    contract,
                ));
            }
        }
        None => {}
    }
}

/// Matches Spec 534 FR-004 exactly as Traverse's own runtime
/// `crates/traverse-runtime/src/events/validation.rs::is_fact_type`
/// implements it: the name must end with `ed`, no irregular-verb
/// allowance. v1.0.0 of this spec used a looser, hyphen-segment-plus-allow-list
/// check that could accept a name Traverse's own runtime would reject
/// (NFR-006) -- corrected here.
fn validate_past_tense_name(contract: &EventContract, errors: &mut Vec<EventProductValidationError>) {
    if !contract.name.ends_with("ed") {
        errors.push(event_product_error(
            EventProductErrorCode::NonPastTenseName,
            "$.name",
            &format!(
                "'{}' does not end in '-ed'; ECCA event names MUST describe a past-tense fact",
                contract.name
            ),
            contract,
        ));
    }
}

fn validate_cloud_events_mapping(
    contract: &EventContract,
    source: &str,
    subject_field: Option<&str>,
    errors: &mut Vec<EventProductValidationError>,
) {
    if source.is_empty() {
        errors.push(event_product_error(
            EventProductErrorCode::MissingCloudEventsSource,
            "$.cloud_events_source",
            "cloud_events_source is required",
            contract,
        ));
    }

    if let Some(subject_field) = subject_field {
        let declared_properties = declared_payload_properties(contract);
        if !declared_properties.iter().any(|property| property == subject_field) {
            errors.push(event_product_error(
                EventProductErrorCode::InvalidCloudEventsSubjectField,
                "$.cloud_events_subject_field",
                &format!("'{subject_field}' is not a declared top-level payload property"),
                contract,
            ));
        }
    }
}

fn validate_delivery_semantics(
    contract: &EventContract,
    deduplication_id_field: &str,
    correlation_id_field: &str,
    errors: &mut Vec<EventProductValidationError>,
) {
    if deduplication_id_field.is_empty() {
        errors.push(event_product_error(
            EventProductErrorCode::MissingDeduplicationIdField,
            "$.deduplication_id_field",
            "deduplication_id_field is required",
            contract,
        ));
    }

    if correlation_id_field.is_empty() {
        errors.push(event_product_error(
            EventProductErrorCode::MissingCorrelationIdField,
            "$.correlation_id_field",
            "correlation_id_field is required",
            contract,
        ));
    }
}

fn validate_retention_policy(
    contract: &EventContract,
    retention_policy: &str,
    errors: &mut Vec<EventProductValidationError>,
) {
    if retention_policy.is_empty() {
        errors.push(event_product_error(
            EventProductErrorCode::MissingRetentionPolicy,
            "$.retention_policy",
            "retention_policy is required",
            contract,
        ));
    }
}

fn remediation_for(code: EventProductErrorCode) -> &'static str {
    match code {
        EventProductErrorCode::MissingSupportRoute => {
            "add a support_route pointing at a stable, owned support destination"
        }
        EventProductErrorCode::InvalidSupportRoute => "change support_route to start with https://",
        EventProductErrorCode::MissingFieldClassification => {
            "add a field_classifications entry for every top-level payload property"
        }
        EventProductErrorCode::UnexpectedFieldClassification => {
            "remove the field_classifications entry, or add the property to payload.schema first"
        }
        EventProductErrorCode::DuplicateFieldClassification => {
            "remove the duplicate field_classifications entry"
        }
        EventProductErrorCode::MissingReplacement => {
            "add a replacement pointing at the successor event before deprecating or retiring"
        }
        EventProductErrorCode::UnexpectedReplacement => {
            "remove replacement, or set lifecycle to deprecated/retired first"
        }
        EventProductErrorCode::InvalidReplacement => {
            "point replacement at a different, non-empty event_id/version"
        }
        EventProductErrorCode::NonPastTenseName => {
            "rename the event so it ends in a past-tense fact (e.g. '-created', '-issued')"
        }
        EventProductErrorCode::ImmutableDescriptorConflict => {
            "publish a new version instead of changing a published descriptor"
        }
        EventProductErrorCode::MissingCloudEventsSource => "add a non-empty cloud_events_source",
        EventProductErrorCode::InvalidCloudEventsSubjectField => {
            "point cloud_events_subject_field at a declared payload property, or leave it unset"
        }
        EventProductErrorCode::MissingDeduplicationIdField => {
            "add a deduplication_id_field naming what deduplicates this event"
        }
        EventProductErrorCode::MissingCorrelationIdField => {
            "add a correlation_id_field naming what correlates this event"
        }
        EventProductErrorCode::MissingRetentionPolicy => "add a non-empty retention_policy statement",
    }
}

fn event_product_error(
    code: EventProductErrorCode,
    path: &str,
    message: &str,
    contract: &EventContract,
) -> EventProductValidationError {
    EventProductValidationError {
        code,
        path: path.to_string(),
        message: message.to_string(),
        severity: ErrorSeverity::Error,
        remediation: remediation_for(code).to_string(),
        contract_id: contract.id.clone(),
        contract_version: contract.version.clone(),
        governing_spec: EVENT_PRODUCT_GOVERNING_SPEC,
    }
}

type EventProductKey = (RegistryScope, String, String);

/// Stores validated [`EventProductDescriptor`] records and indexes the
/// declared producer/consumer relationships already present on each
/// descriptor's `contract.publishers`/`contract.subscribers` -- this does
/// **not** introduce a second, divergent declared-relationship model
/// (spec 016 FR-012); it indexes what `012`'s `EventContract` already
/// declares so it can be looked up by capability without reparsing raw
/// contract JSON.
#[derive(Debug, Clone, Default)]
pub struct EventProductRegistry {
    descriptors: BTreeMap<EventProductKey, EventProductDescriptor>,
    publishers_index: BTreeMap<String, BTreeSet<EventProductKey>>,
    subscribers_index: BTreeMap<String, BTreeSet<EventProductKey>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventProductRegistration {
    pub scope: RegistryScope,
    pub descriptor: EventProductDescriptor,
}

impl EventProductRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers one ECCA event-product descriptor.
    ///
    /// # Errors
    ///
    /// Returns [`EventProductValidationFailure`] when the descriptor fails
    /// `validate_event_product_descriptor`, including an immutable-content
    /// conflict against a previously registered descriptor for the same
    /// `(scope, id, version)`.
    pub fn register(
        &mut self,
        request: EventProductRegistration,
    ) -> Result<(), EventProductValidationFailure> {
        let EventProductRegistration { scope, descriptor } = request;
        let key = event_product_key(scope, &descriptor);
        let existing = self.descriptors.get(&key);
        validate_event_product_descriptor(&descriptor, existing)?;

        if existing == Some(&descriptor) {
            return Ok(());
        }

        self.index_declared_relationships(&key, &descriptor);
        self.descriptors.insert(key, descriptor);
        Ok(())
    }

    fn index_declared_relationships(
        &mut self,
        key: &EventProductKey,
        descriptor: &EventProductDescriptor,
    ) {
        for publisher in &descriptor.contract.publishers {
            self.publishers_index
                .entry(publisher.capability_id.clone())
                .or_default()
                .insert(key.clone());
        }
        for subscriber in &descriptor.contract.subscribers {
            self.subscribers_index
                .entry(subscriber.capability_id.clone())
                .or_default()
                .insert(key.clone());
        }
    }

    #[must_use]
    pub fn find_exact(
        &self,
        scope: RegistryScope,
        id: &str,
        version: &str,
    ) -> Option<&EventProductDescriptor> {
        self.descriptors
            .get(&(scope, id.to_string(), version.to_string()))
    }

    /// Deterministic discovery across scopes, honoring the same
    /// public/private precedence rules as `012`'s `EventRegistry::discover`.
    #[must_use]
    pub fn discover(&self, lookup_scope: LookupScope) -> Vec<&EventProductDescriptor> {
        let mut results = Vec::new();
        let mut shadowed = BTreeSet::new();

        for &scope in crate::events::lookup_order(lookup_scope) {
            let entries = self
                .descriptors
                .iter()
                .filter(|((entry_scope, _, _), _)| *entry_scope == scope);

            for ((_, id, version), descriptor) in entries {
                if lookup_scope == LookupScope::PreferPrivate
                    && scope == RegistryScope::Public
                    && shadowed.contains(&(id.clone(), version.clone()))
                {
                    continue;
                }

                if scope == RegistryScope::Private {
                    shadowed.insert((id.clone(), version.clone()));
                }

                results.push(descriptor);
            }
        }

        results.sort_by(|left, right| {
            left.contract
                .id
                .cmp(&right.contract.id)
                .then_with(|| {
                    crate::events::compare_versions(&right.contract.version, &left.contract.version)
                })
                .then_with(|| left.contract.namespace.cmp(&right.contract.namespace))
        });
        results
    }

    /// Declared events one capability publishes, ordered deterministically
    /// by `(scope, id, version)`.
    #[must_use]
    pub fn declared_publishes(&self, capability_id: &str) -> Vec<&EventProductDescriptor> {
        self.publishers_index
            .get(capability_id)
            .into_iter()
            .flatten()
            .filter_map(|key| self.descriptors.get(key))
            .collect()
    }

    /// Declared events one capability consumes, ordered deterministically
    /// by `(scope, id, version)`.
    #[must_use]
    pub fn declared_consumes(&self, capability_id: &str) -> Vec<&EventProductDescriptor> {
        self.subscribers_index
            .get(capability_id)
            .into_iter()
            .flatten()
            .filter_map(|key| self.descriptors.get(key))
            .collect()
    }
}

fn event_product_key(scope: RegistryScope, descriptor: &EventProductDescriptor) -> EventProductKey {
    (
        scope,
        descriptor.contract.id.clone(),
        descriptor.contract.version.clone(),
    )
}

/// Catalog/discovery filters over registered event products (spec 016
/// FR-014, matching Spec 534 FR-018's exact dimension list: event,
/// producer, consumer, domain, owner, lifecycle, classification, and
/// payload-field metadata, plus `event_type` for FR-018's "type"
/// dimension). Every field is optional; an absent field imposes no
/// constraint, so an all-`None` query returns everything `discover` would.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventProductCatalogQuery {
    pub event_id: Option<String>,
    /// Matches when the event's declared `publishers` includes this capability.
    pub producer_capability_id: Option<String>,
    /// Matches when the event's declared `subscribers` includes this capability.
    pub consumer_capability_id: Option<String>,
    pub domain: Option<String>,
    pub owner_team: Option<String>,
    pub lifecycle: Option<Lifecycle>,
    pub classification: Option<DataClassification>,
    pub event_type: Option<EventType>,
    /// Matches when the event's payload schema declares this top-level property.
    pub payload_field: Option<String>,
}

impl EventProductRegistry {
    /// Deterministic catalog/discovery search: filters `discover`'s
    /// already-deterministic, precedence-ordered results by every
    /// non-`None` field in `query`.
    #[must_use]
    pub fn catalog_search(
        &self,
        lookup_scope: LookupScope,
        query: &EventProductCatalogQuery,
    ) -> Vec<&EventProductDescriptor> {
        self.discover(lookup_scope)
            .into_iter()
            .filter(|descriptor| matches_query(descriptor, query))
            .collect()
    }
}

fn matches_query(descriptor: &EventProductDescriptor, query: &EventProductCatalogQuery) -> bool {
    let contract = &descriptor.contract;

    if let Some(event_id) = &query.event_id
        && &contract.id != event_id
    {
        return false;
    }

    if let Some(capability_id) = &query.producer_capability_id
        && !contract
            .publishers
            .iter()
            .any(|reference| &reference.capability_id == capability_id)
    {
        return false;
    }

    if let Some(capability_id) = &query.consumer_capability_id
        && !contract
            .subscribers
            .iter()
            .any(|reference| &reference.capability_id == capability_id)
    {
        return false;
    }

    if let Some(domain) = &query.domain
        && &contract.classification.domain != domain
    {
        return false;
    }

    if let Some(owner_team) = &query.owner_team
        && &contract.owner.team != owner_team
    {
        return false;
    }

    if let Some(lifecycle) = &query.lifecycle
        && &contract.lifecycle != lifecycle
    {
        return false;
    }

    if let Some(classification) = &query.classification
        && !descriptor
            .field_classifications
            .iter()
            .any(|entry| &entry.classification == classification)
    {
        return false;
    }

    if let Some(event_type) = &query.event_type
        && &contract.classification.event_type != event_type
    {
        return false;
    }

    if let Some(payload_field) = &query.payload_field
        && !declared_payload_properties(contract)
            .iter()
            .any(|property| property == payload_field)
    {
        return false;
    }

    true
}
