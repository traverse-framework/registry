use crate::{LookupScope, RegistryScope};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use traverse_contracts::{ErrorSeverity, EventContract, Lifecycle};

/// Governing spec for this module: `specs/016-ecca-event-product-adoption`.
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
    pub field_classifications: Vec<FieldClassification>,
    pub replacement: Option<EventProductReplacement>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldClassification {
    pub field_path: String,
    pub classification: DataClassification,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataClassification {
    Public,
    Internal,
    Confidential,
    Restricted,
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

type EventProductKey = (RegistryScope, String, String);

/// Stores validated [`EventProductDescriptor`] records and indexes the
/// declared producer/consumer relationships already present on each
/// descriptor's `contract.publishers`/`contract.subscribers` -- this does
/// **not** introduce a second, divergent declared-relationship model
/// (spec 016 FR-008); it indexes what `012`'s `EventContract` already
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
/// FR-010). Every field is optional; an absent field imposes no
/// constraint, so an all-`None` query returns everything `discover` would.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventProductCatalogQuery {
    pub event_id: Option<String>,
    pub capability_id: Option<String>,
    pub domain: Option<String>,
    pub owner_team: Option<String>,
    pub lifecycle: Option<Lifecycle>,
    pub classification: Option<DataClassification>,
}

impl EventProductRegistry {
    /// Deterministic catalog/discovery search: filters `discover`'s
    /// already-deterministic, precedence-ordered results by every
    /// non-`None` field in `query`. `capability_id` matches either a
    /// declared publisher or a declared subscriber. `classification`
    /// matches when any of the descriptor's field classifications is at
    /// that level.
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

    if let Some(capability_id) = &query.capability_id {
        let is_publisher = contract
            .publishers
            .iter()
            .any(|reference| &reference.capability_id == capability_id);
        let is_subscriber = contract
            .subscribers
            .iter()
            .any(|reference| &reference.capability_id == capability_id);
        if !is_publisher && !is_subscriber {
            return false;
        }
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

    if let Some(classification) = &query.classification {
        let has_classification = descriptor
            .field_classifications
            .iter()
            .any(|entry| &entry.classification == classification);
        if !has_classification {
            return false;
        }
    }

    true
}
