//! Real, deterministic implementation of the `doc-approval.analyze` capability.
//!
//! No model, network, randomness, or host state dependency -- matches the
//! contract's own constraints. This replaces the fixed-output fixture from
//! `1.0.1` (see registry#69, registry#79, docs/decision-log.md entries 34-35):
//! output genuinely depends on the input `document` string via simple
//! keyword/heuristic rules, not a single hardcoded response.

use serde_json::{json, Value};
use std::io::{self, Read, Write};

fn classify_doc_type(document: &str) -> &'static str {
    let lower = document.to_lowercase();
    if lower.contains("invoice") {
        "invoice"
    } else if lower.contains("purchase order") || lower.contains(" po#") || lower.contains(" po ") {
        "purchase_order"
    } else if lower.contains("receipt") {
        "receipt"
    } else if lower.contains("contract") || lower.contains("agreement") {
        "contract"
    } else {
        "general"
    }
}

fn is_capitalized_word(word: &str) -> bool {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) if first.is_uppercase() => chars.all(|c| c.is_lowercase() || c.is_ascii_digit()),
        _ => false,
    }
}

/// Heuristic: two consecutive capitalized words (e.g. "Acme Corp", "Jane Doe")
/// are treated as a named party. Deliberately simple -- a reference-tier,
/// no-model capability, not a production NER system.
fn extract_parties(document: &str) -> Vec<String> {
    let words: Vec<&str> = document.split_whitespace().collect();
    let mut parties = Vec::new();
    let mut i = 0;
    while i + 1 < words.len() {
        let w1 = words[i].trim_matches(|c: char| !c.is_alphanumeric());
        let w2 = words[i + 1].trim_matches(|c: char| !c.is_alphanumeric());
        if is_capitalized_word(w1) && is_capitalized_word(w2) {
            let party = format!("{w1} {w2}");
            if !parties.contains(&party) {
                parties.push(party);
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    parties
}

/// Heuristic: `$` followed by digits/commas/periods is treated as a dollar amount.
fn extract_amounts(document: &str) -> Vec<String> {
    let chars: Vec<char> = document.chars().collect();
    let mut amounts = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' {
            let mut j = i + 1;
            let mut amount = String::from("$");
            while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == ',' || chars[j] == '.') {
                amount.push(chars[j]);
                j += 1;
            }
            if amount.len() > 1 {
                amounts.push(amount);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    amounts
}

fn determine_confidence(doc_type: &str, parties: &[String], amounts: &[String]) -> &'static str {
    let signals =
        u8::from(doc_type != "general") + u8::from(!parties.is_empty()) + u8::from(!amounts.is_empty());
    match signals {
        3 => "high",
        1 | 2 => "medium",
        _ => "low",
    }
}

fn determine_recommendation(confidence: &str) -> &'static str {
    match confidence {
        "high" => "approve",
        "medium" => "manual_review",
        _ => "insufficient_data",
    }
}

fn main() {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .expect("read stdin");

    let parsed: Value = serde_json::from_str(&input).unwrap_or_else(|_| json!({}));
    let document = parsed.get("document").and_then(Value::as_str).unwrap_or("");

    let doc_type = classify_doc_type(document);
    let parties = extract_parties(document);
    let amounts = extract_amounts(document);
    let confidence = determine_confidence(doc_type, &parties, &amounts);
    let recommendation = determine_recommendation(confidence);

    let output = json!({
        "docType": doc_type,
        "parties": parties,
        "amounts": amounts,
        "confidence": confidence,
        "recommendation": recommendation,
    });

    print!("{output}");
    io::stdout().flush().expect("flush stdout");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_invoice_and_extracts_amount() {
        assert_eq!(classify_doc_type("This is an INVOICE for services."), "invoice");
        assert_eq!(extract_amounts("Total due: $1,234.56 net 30."), vec!["$1,234.56"]);
    }

    #[test]
    fn classifies_contract_and_extracts_parties() {
        assert_eq!(
            classify_doc_type("This Agreement is entered into by Acme Corp and Jane Doe."),
            "contract"
        );
        let parties = extract_parties("This Agreement is entered into by Acme Corp and Jane Doe.");
        assert!(parties.contains(&"Acme Corp".to_string()));
        assert!(parties.contains(&"Jane Doe".to_string()));
    }

    #[test]
    fn general_document_with_no_signals_has_low_confidence() {
        assert_eq!(classify_doc_type("just some plain lowercase text"), "general");
        assert_eq!(determine_confidence("general", &[], &[]), "low");
        assert_eq!(determine_recommendation("low"), "insufficient_data");
    }

    #[test]
    fn output_genuinely_differs_across_distinct_inputs() {
        let doc_a = "INVOICE. Bill to: Acme Corp. Total: $500.00";
        let doc_b = "just some plain lowercase text with no signals";

        let doc_type_a = classify_doc_type(doc_a);
        let doc_type_b = classify_doc_type(doc_b);
        assert_ne!(doc_type_a, doc_type_b, "output must depend on input, not be fixed");

        let amounts_a = extract_amounts(doc_a);
        let amounts_b = extract_amounts(doc_b);
        assert_ne!(amounts_a, amounts_b);
    }
}
