//! Observed runtime event-product lineage and drift evidence.
//!
//! Structurally disjoint from [`crate::EventProductDescriptor`] and
//! [`crate::EventProductRegistry`] (spec `016-ecca-event-product-adoption`
//! FR-009): this module has no field, method, or dependency pointing back
//! into descriptor storage or `validate_event_product_descriptor`, so
//! declared-state validation outcomes cannot depend on anything recorded
//! here. `record` only ever *reads* a caller-supplied snapshot of declared
//! capability ids to compute drift -- it never reaches back into the
//! declared registry itself, and nothing here can be called from, or
//! change the result of, descriptor validation.
//!
//! Mirrors `specs/015-runtime-usage-telemetry-resolve-hook`'s hook-isolation
//! shape: observations are supplied only by an external, side-effect-only
//! caller (there is no code path here that manufactures its own runtime
//! evidence), and recording never fails or blocks anything.

/// Whether an observed interaction was a publish or a consume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservedRole {
    Publisher,
    Subscriber,
}

/// One runtime-observed interaction between a capability and an event
/// product. Deliberately carries no payload, credential, or root data --
/// there is no field capable of holding any, by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedEventInteraction {
    pub event_id: String,
    pub event_version: String,
    pub capability_id: String,
    pub role: ObservedRole,
    pub observed_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftKind {
    /// A capability was observed publishing an event it isn't a declared publisher of.
    UndeclaredPublisher,
    /// A capability was observed consuming an event it isn't a declared subscriber of.
    UndeclaredSubscriber,
}

/// Evidence that an observed interaction diverges from the declared
/// relationships for the same event. Carries only identifiers and the
/// observation timestamp -- no payload, credential, or root data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriftEvidence {
    pub kind: DriftKind,
    pub event_id: String,
    pub event_version: String,
    pub capability_id: String,
    pub observed_at: String,
}

/// Append-only store of observed lineage and the drift evidence derived
/// from it. Holds no reference to any declared-state store.
#[derive(Debug, Clone, Default)]
pub struct ObservedLineageStore {
    interactions: Vec<ObservedEventInteraction>,
    drift: Vec<DriftEvidence>,
}

impl ObservedLineageStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one observed interaction. `declared_capability_ids` is a
    /// caller-supplied snapshot (e.g. from
    /// `EventProductRegistry::declared_publishes`/`declared_consumes`) of
    /// which capability ids are declared for this role on this event --
    /// this method never looks that up itself, so it has no dependency on
    /// declared-state storage. Never fails: an external, side-effect-only
    /// caller's observation is always accepted, consistent with
    /// `specs/015-runtime-usage-telemetry-resolve-hook`'s hook-isolation
    /// shape.
    pub fn record(
        &mut self,
        interaction: ObservedEventInteraction,
        declared_capability_ids: &[String],
    ) {
        if !declared_capability_ids.contains(&interaction.capability_id) {
            self.drift.push(DriftEvidence {
                kind: match interaction.role {
                    ObservedRole::Publisher => DriftKind::UndeclaredPublisher,
                    ObservedRole::Subscriber => DriftKind::UndeclaredSubscriber,
                },
                event_id: interaction.event_id.clone(),
                event_version: interaction.event_version.clone(),
                capability_id: interaction.capability_id.clone(),
                observed_at: interaction.observed_at.clone(),
            });
        }

        self.interactions.push(interaction);
    }

    /// Observed interactions for one event version, in recording order.
    #[must_use]
    pub fn interactions_for(&self, event_id: &str, event_version: &str) -> Vec<&ObservedEventInteraction> {
        self.interactions
            .iter()
            .filter(|interaction| {
                interaction.event_id == event_id && interaction.event_version == event_version
            })
            .collect()
    }

    /// Drift evidence for one event version, in recording order.
    #[must_use]
    pub fn drift_for(&self, event_id: &str, event_version: &str) -> Vec<&DriftEvidence> {
        self.drift
            .iter()
            .filter(|evidence| {
                evidence.event_id == event_id && evidence.event_version == event_version
            })
            .collect()
    }
}
