//! Real, deterministic implementation of the `doc-approval.recommend`
//! capability: it's a genuine second-pass check on `doc-approval.analyze`'s
//! output, not an echo of it -- it independently recomputes a confidence
//! level from the actual signal counts (docType/parties/amounts) and takes
//! the more conservative of that and the input's own stated confidence,
//! downgrading the recommendation (with a rationale explaining why) when
//! the input's stated confidence doesn't match what the signals actually
//! support. On every recommendation, emits
//! `doc-approval.recommendation-issued@1.0.0` via `traverse_host::emit_event`.
//! See registry#79, docs/decision-log.md.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use wasi_capability_runtime::{object, Value};

#[cfg(not(test))]
#[link(wasm_import_module = "traverse_host")]
unsafe extern "C" {
    fn emit_event(ptr: i32, len: i32) -> i32;
}

#[cfg(test)]
unsafe fn emit_event(_ptr: i32, _len: i32) -> i32 {
    0
}

fn confidence_rank(confidence: &str) -> u8 {
    match confidence {
        "high" => 2,
        "medium" => 1,
        _ => 0,
    }
}

fn rank_to_confidence(rank: u8) -> &'static str {
    match rank {
        2 => "high",
        1 => "medium",
        _ => "low",
    }
}

fn rank_to_recommendation(rank: u8) -> &'static str {
    match rank {
        2 => "approve",
        1 => "manual_review",
        _ => "insufficient_data",
    }
}

fn emit_recommendation_issued(
    recommendation: &str,
    confidence: &str,
    rationale: &str,
    doc_type: &str,
) {
    let mut payload = Vec::new();
    payload.extend_from_slice(br#"{"event_id":"doc-approval.recommendation-issued","version":"1.0.0","payload":{"docType":""#);
    payload.extend_from_slice(doc_type.as_bytes());
    payload.extend_from_slice(br#"","recommendation":""#);
    payload.extend_from_slice(recommendation.as_bytes());
    payload.extend_from_slice(br#"","confidence":""#);
    payload.extend_from_slice(confidence.as_bytes());
    payload.extend_from_slice(br#"","rationale":""#);
    // Rationale may contain quotes; escape them for JSON.
    for b in rationale.bytes() {
        if b == b'"' || b == b'\\' {
            payload.push(b'\\');
        }
        payload.push(b);
    }
    payload.extend_from_slice(br#""}}"#);
    unsafe {
        let _ = emit_event(payload.as_ptr() as i32, payload.len() as i32);
    }
}

fn recommend(input: Value) -> Value {
    let doc_type = input
        .get("docType")
        .and_then(Value::as_str)
        .unwrap_or("general");
    let parties = input
        .get("parties")
        .map(Value::string_array)
        .unwrap_or_default();
    let amounts = input
        .get("amounts")
        .map(Value::string_array)
        .unwrap_or_default();
    let input_confidence = input
        .get("confidence")
        .and_then(Value::as_str)
        .unwrap_or("low");

    let signal_count = u8::from(doc_type != "general")
        + u8::from(!parties.is_empty())
        + u8::from(!amounts.is_empty());
    let recomputed_rank = signal_count;
    let input_rank = confidence_rank(input_confidence);
    let final_rank = if recomputed_rank < input_rank {
        recomputed_rank
    } else {
        input_rank
    };

    let final_confidence = rank_to_confidence(final_rank);
    let final_recommendation = rank_to_recommendation(final_rank);

    let mut rationale = alloc::format!(
        "docType={doc_type}; {} part{} found; {} amount{} found",
        parties.len(),
        if parties.len() == 1 { "y" } else { "ies" },
        amounts.len(),
        if amounts.len() == 1 { "" } else { "s" }
    );
    if final_rank < input_rank {
        rationale.push_str(&alloc::format!(
            "; downgraded from {input_confidence} confidence because detected signals only support {final_confidence}"
        ));
    } else {
        rationale.push_str(&alloc::format!(
            "; confirmed at {final_confidence} confidence"
        ));
    }

    emit_recommendation_issued(final_recommendation, final_confidence, &rationale, doc_type);

    object(alloc::vec![
        (
            "recommendation",
            Value::String(String::from(final_recommendation))
        ),
        ("rationale", Value::String(rationale)),
        ("confidence", Value::String(String::from(final_confidence))),
    ])
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    wasi_capability_runtime::run_capability(recommend);
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasi_capability_runtime::array_of_strings;

    fn run(doc_type: &str, parties: &[&str], amounts: &[&str], confidence: &str) -> Value {
        let party_strings: alloc::vec::Vec<String> =
            parties.iter().map(|p| String::from(*p)).collect();
        let amount_strings: alloc::vec::Vec<String> =
            amounts.iter().map(|a| String::from(*a)).collect();
        recommend(object(alloc::vec![
            ("docType", Value::String(String::from(doc_type))),
            ("parties", array_of_strings(&party_strings)),
            ("amounts", array_of_strings(&amount_strings)),
            ("confidence", Value::String(String::from(confidence))),
            ("recommendation", Value::String(String::from("approve"))),
        ]))
    }

    #[test]
    fn confirms_a_genuinely_high_confidence_document() {
        let out = run("invoice", &["Acme Corp"], &["$500.00"], "high");
        assert_eq!(
            out.get("recommendation").unwrap().as_str().unwrap(),
            "approve"
        );
        assert_eq!(out.get("confidence").unwrap().as_str().unwrap(), "high");
        assert!(out
            .get("rationale")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("confirmed"));
    }

    #[test]
    fn downgrades_a_mismatched_high_confidence_claim() {
        // Input claims "high" confidence but has zero actual signals -- must be downgraded, not echoed.
        let out = run("general", &[], &[], "high");
        assert_ne!(
            out.get("recommendation").unwrap().as_str().unwrap(),
            "approve"
        );
        assert_eq!(out.get("confidence").unwrap().as_str().unwrap(), "low");
        assert!(out
            .get("rationale")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("downgraded"));
    }

    #[test]
    fn output_genuinely_differs_across_distinct_inputs() {
        let out_a = run("invoice", &["Acme Corp"], &["$500.00"], "high");
        let out_b = run("general", &[], &[], "low");
        assert_ne!(
            out_a.get("recommendation").unwrap().as_str(),
            out_b.get("recommendation").unwrap().as_str(),
            "output must depend on input, not be fixed"
        );
    }
}
