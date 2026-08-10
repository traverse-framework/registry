//! Build-tool WASM binary for registry#105 (child of #103, the capability
//! discovery umbrella). Not a published `capabilities/` capability -- see
//! `Cargo.toml`'s description and `docs/decision-log.md` entry 40.
//!
//! Input: the JSON object `scripts/ci/gather_catalog_data.py` produces by
//! walking `capabilities/**/contract.json`, `personas/**/persona.json`, and
//! `events/**/product.json` (the WASM ABI only allows a single input/single
//! output via `fd_read`/`fd_write`, no filesystem access, so this crate
//! cannot walk those trees itself):
//! `{capabilities: [{deprecated, contract, test_coverage}],
//! personas: [{persona}], events: [{deprecated, product}]}`.
//! Output: one JSON object with a deterministically sorted `capabilities`
//! list (each entry carrying the *entire* source contract, not a hand-picked
//! field subset -- the catalog's per-capability detail page needs "all the
//! infos"), a `search_index` (lowercase token -> sorted
//! `namespace/id@version` references) built from capabilities only, a
//! deterministically sorted `personas` list (specs/017-persona-registry,
//! decision-log entry 53), and a deterministically sorted `events` list
//! (specs/016 FR-014 / registry#160; each entry carries the entire
//! EventProductDescriptor), which registry#106's GitHub Pages template
//! renders and searches client-side.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::vec::Vec;
use wasi_capability_runtime::{object, Value};

fn capability_reference(namespace: &str, id: &str, version: &str) -> String {
    alloc::format!("{namespace}/{id}@{version}")
}

/// Splits ASCII-alphanumeric runs into lowercase tokens; everything else is
/// a separator. Deliberately simple (no stemming, no unicode folding) --
/// this is a substring/keyword index for a small, curated catalog, not a
/// general-purpose search engine.
fn tokenize_into(text: &str, tokens: &mut BTreeSet<String>) {
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            tokens.insert(core::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.insert(current);
    }
}

/// `use_cases` (and other nested contract fields) aren't walked field by
/// field for tokenizing -- pull every string leaf out of whatever JSON
/// shape is there, so a schema change elsewhere in the contract doesn't
/// silently stop contributing to search.
fn append_text_leaves(value: &Value, out: &mut String) {
    match value {
        Value::String(s) => {
            out.push(' ');
            out.push_str(s);
        }
        Value::Array(items) => {
            for item in items {
                append_text_leaves(item, out);
            }
        }
        Value::Object(fields) => {
            for (_, v) in fields {
                append_text_leaves(v, out);
            }
        }
        _ => {}
    }
}

/// Builds the personas[] output list from the personas half of the input
/// (specs/017-persona-registry, decision-log entry 53). Deliberately not
/// folded into the capability search_index -- personas are reached via a
/// dedicated list view and direct links from a use case's persona_ref, not
/// free-text search, so this stays a simple pass-through/sort, mirroring
/// capabilities' own "carry the entire record" rule without capabilities'
/// search-indexing complexity.
fn build_personas(persona_records: &[Value]) -> Vec<Value> {
    let mut personas: Vec<Value> = Vec::new();

    for record in persona_records {
        let persona = match record.get("persona") {
            Some(p) => p,
            None => continue,
        };
        let id = persona.get("id").and_then(Value::as_str).unwrap_or("");
        let version = persona.get("version").and_then(Value::as_str).unwrap_or("");
        if id.is_empty() || version.is_empty() {
            continue;
        }
        personas.push(object(alloc::vec![
            ("reference", Value::String(alloc::format!("{id}@{version}"))),
            ("persona", persona.clone()),
        ]));
    }

    personas.sort_by(|a, b| {
        let reference_a = a.get("reference").and_then(Value::as_str).unwrap_or("");
        let reference_b = b.get("reference").and_then(Value::as_str).unwrap_or("");
        reference_a.cmp(reference_b)
    });

    personas
}

/// Builds the events[] output list from the events half of the input
/// (specs/016 FR-014 / registry#160). Same pass-through/sort pattern as
/// personas -- event products are reached via a dedicated filtered list
/// view and direct links, not the capability search_index.
fn build_events(event_records: &[Value]) -> Vec<Value> {
    let mut events: Vec<Value> = Vec::new();

    for record in event_records {
        let product = match record.get("product") {
            Some(p) => p,
            None => continue,
        };
        let contract = match product.get("contract") {
            Some(c) => c,
            None => continue,
        };
        let namespace = contract.get("namespace").and_then(Value::as_str).unwrap_or("");
        let id = contract.get("id").and_then(Value::as_str).unwrap_or("");
        let version = contract.get("version").and_then(Value::as_str).unwrap_or("");
        if namespace.is_empty() || id.is_empty() || version.is_empty() {
            continue;
        }
        let deprecated = record
            .get("deprecated")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        events.push(object(alloc::vec![
            (
                "reference",
                Value::String(capability_reference(namespace, id, version)),
            ),
            ("deprecated", Value::Bool(deprecated)),
            ("product", product.clone()),
        ]));
    }

    events.sort_by(|a, b| {
        let reference_a = a.get("reference").and_then(Value::as_str).unwrap_or("");
        let reference_b = b.get("reference").and_then(Value::as_str).unwrap_or("");
        reference_a.cmp(reference_b)
    });

    events
}

fn build_catalog(input: &Value) -> Value {
    let records = input.get("capabilities").and_then(Value::as_array).unwrap_or(&[]);
    let persona_records = input.get("personas").and_then(Value::as_array).unwrap_or(&[]);
    let event_records = input.get("events").and_then(Value::as_array).unwrap_or(&[]);
    let personas = build_personas(persona_records);
    let events = build_events(event_records);

    let mut capabilities: Vec<Value> = Vec::new();
    let mut index: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for record in records {
        let contract = match record.get("contract") {
            Some(c) => c,
            None => continue,
        };
        let namespace = contract.get("namespace").and_then(Value::as_str).unwrap_or("");
        let id = contract.get("id").and_then(Value::as_str).unwrap_or("");
        let version = contract.get("version").and_then(Value::as_str).unwrap_or("");
        if namespace.is_empty() || id.is_empty() || version.is_empty() {
            // Gather script always populates these for a real contract;
            // skip rather than emit an unusable catalog entry.
            continue;
        }
        let deprecated = record
            .get("deprecated")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let test_coverage = record.get("test_coverage").cloned().unwrap_or(Value::Null);

        let reference = capability_reference(namespace, id, version);

        // Search text is namespace/id/summary/description/use_cases only --
        // deliberately not the whole contract (inputs/outputs schemas,
        // provenance, etc. would just add noise to token matches).
        let mut text = String::new();
        text.push_str(namespace);
        text.push(' ');
        text.push_str(id);
        if let Some(summary) = contract.get("summary").and_then(Value::as_str) {
            text.push(' ');
            text.push_str(summary);
        }
        if let Some(description) = contract.get("description").and_then(Value::as_str) {
            text.push(' ');
            text.push_str(description);
        }
        if let Some(use_cases) = contract.get("use_cases") {
            append_text_leaves(use_cases, &mut text);
        }

        let mut tokens = BTreeSet::new();
        tokenize_into(&text, &mut tokens);
        for token in tokens {
            index.entry(token).or_default().insert(reference.clone());
        }

        capabilities.push(object(alloc::vec![
            ("reference", Value::String(reference)),
            ("deprecated", Value::Bool(deprecated)),
            ("contract", contract.clone()),
            ("test_coverage", test_coverage),
        ]));
    }

    capabilities.sort_by(|a, b| {
        let reference_a = a.get("reference").and_then(Value::as_str).unwrap_or("");
        let reference_b = b.get("reference").and_then(Value::as_str).unwrap_or("");
        reference_a.cmp(reference_b)
    });

    let search_index = Value::Object(
        index
            .into_iter()
            .map(|(token, refs)| {
                (
                    token,
                    Value::Array(refs.into_iter().map(Value::String).collect()),
                )
            })
            .collect(),
    );

    object(alloc::vec![
        ("capabilities", Value::Array(capabilities)),
        ("personas", Value::Array(personas)),
        ("events", Value::Array(events)),
        ("search_index", search_index),
    ])
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    wasi_capability_runtime::run_capability(|input| build_catalog(&input));
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn contract(namespace: &str, id: &str, version: &str, summary: &str, description: &str) -> Value {
        object(vec![
            ("namespace", Value::String(String::from(namespace))),
            ("id", Value::String(String::from(id))),
            ("version", Value::String(String::from(version))),
            ("summary", Value::String(String::from(summary))),
            ("description", Value::String(String::from(description))),
            ("use_cases", Value::Null),
        ])
    }

    fn record(contract_value: Value, deprecated: bool) -> Value {
        object(vec![
            ("deprecated", Value::Bool(deprecated)),
            ("contract", contract_value),
        ])
    }

    fn capabilities_input(records: Vec<Value>) -> Value {
        object(vec![
            ("capabilities", Value::Array(records)),
            ("personas", Value::Array(vec![])),
            ("events", Value::Array(vec![])),
        ])
    }

    fn event_product(namespace: &str, id: &str, version: &str, summary: &str, exposure: &str) -> Value {
        object(vec![
            (
                "contract",
                object(vec![
                    ("namespace", Value::String(String::from(namespace))),
                    ("id", Value::String(String::from(id))),
                    ("version", Value::String(String::from(version))),
                    ("summary", Value::String(String::from(summary))),
                    ("lifecycle", Value::String(String::from("active"))),
                    (
                        "owner",
                        object(vec![("team", Value::String(String::from("loop")))]),
                    ),
                    (
                        "classification",
                        object(vec![("domain", Value::String(String::from("core.action-item")))]),
                    ),
                    ("publishers", Value::Array(vec![])),
                    ("subscribers", Value::Array(vec![])),
                ]),
            ),
            ("exposure", Value::String(String::from(exposure))),
            ("support_route", Value::String(String::from("https://support.traverse.dev/events"))),
            ("field_classifications", Value::Array(vec![])),
        ])
    }

    fn event_record(product_value: Value, deprecated: bool) -> Value {
        object(vec![
            ("deprecated", Value::Bool(deprecated)),
            ("product", product_value),
        ])
    }

    fn events_input(records: Vec<Value>) -> Value {
        object(vec![
            ("capabilities", Value::Array(vec![])),
            ("personas", Value::Array(vec![])),
            ("events", Value::Array(records)),
        ])
    }

    fn persona(id: &str, version: &str, name: &str) -> Value {
        object(vec![
            ("id", Value::String(String::from(id))),
            ("version", Value::String(String::from(version))),
            ("name", Value::String(String::from(name))),
            ("summary", Value::String(String::from("summary"))),
            ("description", Value::String(String::from("description"))),
            ("distinguished_from", Value::Array(vec![])),
        ])
    }

    fn persona_record(persona_value: Value) -> Value {
        object(vec![("persona", persona_value)])
    }

    fn personas_input(records: Vec<Value>) -> Value {
        object(vec![
            ("capabilities", Value::Array(vec![])),
            ("personas", Value::Array(records)),
            ("events", Value::Array(vec![])),
        ])
    }

    #[test]
    fn builds_sorted_capability_list_and_reference() {
        let input = capabilities_input(vec![
            record(
                contract(
                    "validation",
                    "validation.validate-luhn",
                    "1.0.0",
                    "Checksum-format validation",
                    "Validates card-number-shaped strings via the Luhn algorithm",
                ),
                false,
            ),
            record(
                contract(
                    "doc-approval",
                    "doc-approval.analyze",
                    "1.2.0",
                    "Analyze a document for approval signals",
                    "Extracts approving parties from document text",
                ),
                false,
            ),
        ]);

        let catalog = build_catalog(&input);
        let capabilities = catalog.get("capabilities").unwrap().as_array().unwrap();
        assert_eq!(capabilities.len(), 2);
        // "doc-approval/..." sorts before "validation/..." lexicographically.
        assert_eq!(
            capabilities[0].get("reference").unwrap().as_str(),
            Some("doc-approval/doc-approval.analyze@1.2.0")
        );
        assert_eq!(
            capabilities[1].get("reference").unwrap().as_str(),
            Some("validation/validation.validate-luhn@1.0.0")
        );
    }

    #[test]
    fn full_contract_is_passed_through_verbatim() {
        let source_contract = contract(
            "validation",
            "validation.validate-luhn",
            "1.0.0",
            "Checksum-format validation",
            "Validates card-number-shaped strings via the Luhn algorithm",
        );
        let input = capabilities_input(vec![record(source_contract.clone(), false)]);

        let catalog = build_catalog(&input);
        let capabilities = catalog.get("capabilities").unwrap().as_array().unwrap();
        assert_eq!(capabilities[0].get("contract").unwrap(), &source_contract);
    }

    #[test]
    fn test_coverage_is_passed_through_when_present_and_null_when_absent() {
        let with_coverage = object(vec![
            ("deprecated", Value::Bool(false)),
            (
                "contract",
                contract("core", "core.a", "1.0.0", "alpha", "alpha capability"),
            ),
            (
                "test_coverage",
                object(vec![
                    ("lines_percent", Value::Number(98.8)),
                    ("functions_percent", Value::Number(100.0)),
                    ("regions_percent", Value::Number(99.3)),
                    ("test_count", Value::Number(5.0)),
                ]),
            ),
        ]);
        let without_coverage = record(
            contract("core", "core.b", "1.0.0", "bravo", "bravo capability"),
            false,
        );

        let catalog = build_catalog(&capabilities_input(vec![with_coverage, without_coverage]));
        let capabilities = catalog.get("capabilities").unwrap().as_array().unwrap();

        let a = capabilities.iter().find(|c| c.get("reference").unwrap().as_str() == Some("core/core.a@1.0.0")).unwrap();
        assert_eq!(a.get("test_coverage").unwrap().get("test_count").unwrap().as_f64(), Some(5.0));

        let b = capabilities.iter().find(|c| c.get("reference").unwrap().as_str() == Some("core/core.b@1.0.0")).unwrap();
        assert_eq!(b.get("test_coverage").unwrap(), &Value::Null);
    }

    #[test]
    fn search_index_maps_token_to_matching_references_only() {
        let input = capabilities_input(vec![
            record(
                contract(
                    "validation",
                    "validation.validate-luhn",
                    "1.0.0",
                    "Luhn checksum validation",
                    "Card-number-shaped string check",
                ),
                false,
            ),
            record(
                contract(
                    "formatting",
                    "formatting.format-currency",
                    "1.0.0",
                    "Currency formatting",
                    "Formats a numeric amount as a localized currency string",
                ),
                false,
            ),
        ]);

        let catalog = build_catalog(&input);
        let index = catalog.get("search_index").unwrap();

        let luhn_refs = index.get("luhn").unwrap().as_array().unwrap();
        assert_eq!(luhn_refs.len(), 1);
        assert_eq!(
            luhn_refs[0].as_str(),
            Some("validation/validation.validate-luhn@1.0.0")
        );

        let currency_refs = index.get("currency").unwrap().as_array().unwrap();
        assert_eq!(currency_refs.len(), 1);
        assert_eq!(
            currency_refs[0].as_str(),
            Some("formatting/formatting.format-currency@1.0.0")
        );

        assert!(index.get("nonexistent-token").is_none());
    }

    #[test]
    fn deprecated_flag_is_preserved_not_filtered() {
        let input = capabilities_input(vec![record(
            contract(
                "validation",
                "validation.validate-email",
                "1.0.0",
                "Email validation",
                "Validates email address format",
            ),
            true,
        )]);

        let catalog = build_catalog(&input);
        let capabilities = catalog.get("capabilities").unwrap().as_array().unwrap();
        assert_eq!(capabilities.len(), 1);
        assert_eq!(capabilities[0].get("deprecated").unwrap().as_bool(), Some(true));
    }

    #[test]
    fn malformed_record_missing_contract_is_skipped() {
        let input = capabilities_input(vec![object(vec![("deprecated", Value::Bool(false))])]);

        let catalog = build_catalog(&input);
        let capabilities = catalog.get("capabilities").unwrap().as_array().unwrap();
        assert!(capabilities.is_empty());
    }

    #[test]
    fn malformed_contract_missing_identity_is_skipped() {
        let input = capabilities_input(vec![record(
            object(vec![(
                "summary",
                Value::String(String::from("no namespace/id/version")),
            )]),
            false,
        )]);

        let catalog = build_catalog(&input);
        let capabilities = catalog.get("capabilities").unwrap().as_array().unwrap();
        assert!(capabilities.is_empty());
    }

    #[test]
    fn empty_input_produces_empty_catalog_not_a_panic() {
        let catalog = build_catalog(&capabilities_input(Vec::new()));
        assert!(catalog.get("capabilities").unwrap().as_array().unwrap().is_empty());
        assert!(catalog.get("personas").unwrap().as_array().unwrap().is_empty());
        assert!(catalog.get("events").unwrap().as_array().unwrap().is_empty());
        assert!(matches!(catalog.get("search_index"), Some(Value::Object(fields)) if fields.is_empty()));
    }

    #[test]
    fn output_genuinely_differs_across_distinct_inputs() {
        let one = build_catalog(&capabilities_input(vec![record(
            contract("core", "core.a", "1.0.0", "alpha", "alpha capability"),
            false,
        )]));
        let two = build_catalog(&capabilities_input(vec![record(
            contract("core", "core.b", "1.0.0", "bravo", "bravo capability"),
            false,
        )]));
        assert_ne!(one, two, "output must depend on input, not be fixed");
    }

    #[test]
    fn builds_sorted_persona_list_and_reference() {
        let input = personas_input(vec![
            persona_record(persona("signup-form-developer", "1.0.0", "Signup Form Developer")),
            persona_record(persona("accounts-payable-clerk", "1.0.0", "Accounts-Payable Clerk")),
        ]);

        let catalog = build_catalog(&input);
        let personas = catalog.get("personas").unwrap().as_array().unwrap();
        assert_eq!(personas.len(), 2);
        // "accounts-payable-clerk@..." sorts before "signup-form-developer@..." lexicographically.
        assert_eq!(personas[0].get("reference").unwrap().as_str(), Some("accounts-payable-clerk@1.0.0"));
        assert_eq!(personas[1].get("reference").unwrap().as_str(), Some("signup-form-developer@1.0.0"));
    }

    #[test]
    fn full_persona_is_passed_through_verbatim() {
        let source_persona = persona("meeting-organizer", "1.0.0", "Meeting Organizer");
        let input = personas_input(vec![persona_record(source_persona.clone())]);

        let catalog = build_catalog(&input);
        let personas = catalog.get("personas").unwrap().as_array().unwrap();
        assert_eq!(personas[0].get("persona").unwrap(), &source_persona);
    }

    #[test]
    fn malformed_persona_record_missing_persona_is_skipped() {
        let input = personas_input(vec![object(vec![("not_persona", Value::Bool(true))])]);
        let catalog = build_catalog(&input);
        assert!(catalog.get("personas").unwrap().as_array().unwrap().is_empty());
    }

    #[test]
    fn malformed_persona_missing_identity_is_skipped() {
        let input = personas_input(vec![persona_record(object(vec![(
            "name",
            Value::String(String::from("no id or version")),
        )]))]);
        let catalog = build_catalog(&input);
        assert!(catalog.get("personas").unwrap().as_array().unwrap().is_empty());
    }

    #[test]
    fn capabilities_and_personas_are_independent() {
        let input = object(vec![
            (
                "capabilities",
                Value::Array(vec![record(
                    contract("core", "core.a", "1.0.0", "alpha", "alpha capability"),
                    false,
                )]),
            ),
            (
                "personas",
                Value::Array(vec![persona_record(persona("meeting-organizer", "1.0.0", "Meeting Organizer"))]),
            ),
            ("events", Value::Array(vec![])),
        ]);

        let catalog = build_catalog(&input);
        assert_eq!(catalog.get("capabilities").unwrap().as_array().unwrap().len(), 1);
        assert_eq!(catalog.get("personas").unwrap().as_array().unwrap().len(), 1);
        assert!(catalog.get("events").unwrap().as_array().unwrap().is_empty());
    }

    #[test]
    fn builds_sorted_event_list_and_reference() {
        let input = events_input(vec![
            event_record(
                event_product(
                    "core",
                    "core.action-item.status-transitioned",
                    "1.0.0",
                    "Status transitioned",
                    "internal",
                ),
                false,
            ),
            event_record(
                event_product(
                    "content.comments",
                    "content.comments.comment-draft-created",
                    "1.0.0",
                    "Draft created",
                    "internal",
                ),
                false,
            ),
        ]);

        let catalog = build_catalog(&input);
        let events = catalog.get("events").unwrap().as_array().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0].get("reference").unwrap().as_str(),
            Some("content.comments/content.comments.comment-draft-created@1.0.0")
        );
        assert_eq!(
            events[1].get("reference").unwrap().as_str(),
            Some("core/core.action-item.status-transitioned@1.0.0")
        );
    }

    #[test]
    fn full_event_product_is_passed_through_verbatim() {
        let source = event_product(
            "core",
            "core.action-item.status-transitioned",
            "1.0.0",
            "Status transitioned",
            "internal",
        );
        let input = events_input(vec![event_record(source.clone(), false)]);
        let catalog = build_catalog(&input);
        let events = catalog.get("events").unwrap().as_array().unwrap();
        assert_eq!(events[0].get("product").unwrap(), &source);
        assert_eq!(events[0].get("deprecated").unwrap().as_bool(), Some(false));
    }

    #[test]
    fn malformed_event_record_missing_product_is_skipped() {
        let input = events_input(vec![object(vec![("deprecated", Value::Bool(false))])]);
        let catalog = build_catalog(&input);
        assert!(catalog.get("events").unwrap().as_array().unwrap().is_empty());
    }
}
