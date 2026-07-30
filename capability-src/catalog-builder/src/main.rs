//! Build-tool WASM binary for registry#105 (child of #103, the capability
//! discovery umbrella). Not a published `capabilities/` capability -- see
//! `Cargo.toml`'s description and `docs/decision-log.md` entry 40.
//!
//! Input: the flat JSON array `scripts/ci/gather_catalog_data.py` produces
//! by walking `capabilities/**/contract.json` (the WASM ABI only allows a
//! single input/single output via `fd_read`/`fd_write`, no filesystem
//! access, so this crate cannot walk the tree itself) -- each element is
//! `{deprecated: bool, contract: <full contract.json>}`. Output: one JSON
//! object with a deterministically sorted `capabilities` list (each entry
//! carrying the *entire* source contract, not a hand-picked field subset --
//! the catalog's per-capability detail page needs "all the infos") and a
//! `search_index` (lowercase token -> sorted `namespace/id@version`
//! references), which registry#106's GitHub Pages template renders and
//! searches client-side.

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

fn build_catalog(input: &Value) -> Value {
    let records = input.as_array().unwrap_or(&[]);

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

    #[test]
    fn builds_sorted_capability_list_and_reference() {
        let input = Value::Array(vec![
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
        let input = Value::Array(vec![record(source_contract.clone(), false)]);

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

        let catalog = build_catalog(&Value::Array(vec![with_coverage, without_coverage]));
        let capabilities = catalog.get("capabilities").unwrap().as_array().unwrap();

        let a = capabilities.iter().find(|c| c.get("reference").unwrap().as_str() == Some("core/core.a@1.0.0")).unwrap();
        assert_eq!(a.get("test_coverage").unwrap().get("test_count").unwrap().as_f64(), Some(5.0));

        let b = capabilities.iter().find(|c| c.get("reference").unwrap().as_str() == Some("core/core.b@1.0.0")).unwrap();
        assert_eq!(b.get("test_coverage").unwrap(), &Value::Null);
    }

    #[test]
    fn search_index_maps_token_to_matching_references_only() {
        let input = Value::Array(vec![
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
        let input = Value::Array(vec![record(
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
        let input = Value::Array(vec![object(vec![("deprecated", Value::Bool(false))])]);

        let catalog = build_catalog(&input);
        let capabilities = catalog.get("capabilities").unwrap().as_array().unwrap();
        assert!(capabilities.is_empty());
    }

    #[test]
    fn malformed_contract_missing_identity_is_skipped() {
        let input = Value::Array(vec![record(
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
        let catalog = build_catalog(&Value::Array(Vec::new()));
        assert!(catalog.get("capabilities").unwrap().as_array().unwrap().is_empty());
        assert!(matches!(catalog.get("search_index"), Some(Value::Object(fields)) if fields.is_empty()));
    }

    #[test]
    fn output_genuinely_differs_across_distinct_inputs() {
        let one = build_catalog(&Value::Array(vec![record(
            contract("core", "core.a", "1.0.0", "alpha", "alpha capability"),
            false,
        )]));
        let two = build_catalog(&Value::Array(vec![record(
            contract("core", "core.b", "1.0.0", "bravo", "bravo capability"),
            false,
        )]));
        assert_ne!(one, two, "output must depend on input, not be fixed");
    }
}
