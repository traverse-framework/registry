//! PII/entity redaction for `text.redact-entities`, built directly on
//! `text.detect-entities`' (registry#465) real DistilBERT-NER forward pass
//! -- reused via a path dependency on that crate's `lib.rs`, not
//! duplicated. Second pick from the post-#473 "Five Engines, What Next"
//! downstream-capability review, after `audio.transcribe-speech`'s VAD
//! gate (registry#477): same technique (reuse an already-verified engine
//! rather than a new model), applied to a genuinely different action --
//! "redact" is a different verb from "detect," not a refinement of it, so
//! this ships as a new capability id rather than a version bump.
//!
//! Runs the exact same tokenize -> forward pass -> BIO-decode pipeline as
//! `text.detect-entities`, then instead of returning raw entity spans,
//! walks them in byte-offset order and substitutes a bracketed placeholder
//! (`[PER]`, `[ORG]`, `[LOC]`, `[MISC]`) for each one, copying everything
//! else through unchanged. No new model, no new license/verification
//! burden -- same weight table, same `dslim/distilbert-NER` (Apache-2.0)
//! license record as `text.detect-entities`.
//!
//! Inherits that capability's known limitation: this compact model
//! sometimes tags an interior WordPiece of a multi-piece entity with a
//! fresh `B-` tag instead of a continuing `I-`, which the (shared) BIO
//! merge correctly reports as separate adjacent spans -- so a name like
//! "Acme Corp" can come back partially redacted as `[ORG][ORG]` rather
//! than one `[ORG]`. Faithfully reproduced, not hidden.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::string::String;

use wasi_capability_runtime::{object, Value};

use text_detect_entities_agent as ner;

fn placeholder(label: &str) -> &'static str {
    match label {
        "PER" => "[PER]",
        "ORG" => "[ORG]",
        "LOC" => "[LOC]",
        _ => "[MISC]",
    }
}

/// Builds the redacted string by walking `entities` (already in ascending
/// byte-offset order, non-overlapping -- guaranteed by `decode_entities`'
/// left-to-right BIO merge) and substituting a placeholder for each span,
/// copying everything else through unchanged.
fn redact(text: &str, entities: &[ner::Entity]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;
    for e in entities {
        if e.start < cursor || e.end < e.start || e.end > text.len() {
            continue; // defensive: never trust an out-of-order/invalid span
        }
        out.push_str(&text[cursor..e.start]);
        out.push_str(placeholder(&e.label));
        cursor = e.end;
    }
    out.push_str(&text[cursor..]);
    out
}

fn redact_text(model: &ner::Model, text: &str) -> String {
    let tokens = ner::encode(model, text);
    let logits = ner::forward_logits(model, &tokens);
    let entities = ner::decode_entities(&tokens, &logits);
    redact(text, &entities)
}

fn run_request(model: &ner::Model, input: Value) -> Value {
    let text = input.get("text").and_then(Value::as_str).unwrap_or("");
    let redacted_text = redact_text(model, text);
    object(alloc::vec![("redacted_text", Value::String(redacted_text))])
}

#[cfg(all(feature = "full-model", not(test)))]
fn run(input: Value) -> Value {
    let model = ner::load_production_model();
    run_request(&model, input)
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    #[cfg(feature = "full-model")]
    {
        wasi_capability_runtime::run_capability(run);
    }
    #[cfg(not(feature = "full-model"))]
    {
        // Release builds must enable `full-model`.
        loop {
            core::arch::wasm32::unreachable()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> ner::Model<'static> {
        ner::fixture_model()
    }

    #[test]
    fn placeholder_maps_known_labels_and_defaults_others_to_misc() {
        assert_eq!(placeholder("PER"), "[PER]");
        assert_eq!(placeholder("ORG"), "[ORG]");
        assert_eq!(placeholder("LOC"), "[LOC]");
        assert_eq!(placeholder("MISC"), "[MISC]");
        assert_eq!(placeholder("anything-else"), "[MISC]");
    }

    #[test]
    fn redact_substitutes_a_single_span() {
        let text = "Hello Sarah Chen today";
        let entities = alloc::vec![ner::Entity {
            label: "PER".to_string(),
            start: 6,
            end: 16,
        }];
        assert_eq!(redact(text, &entities), "Hello [PER] today");
    }

    #[test]
    fn redact_substitutes_multiple_non_overlapping_spans_in_order() {
        let text = "Sarah Chen works at Acme Corp";
        let entities = alloc::vec![
            ner::Entity {
                label: "PER".to_string(),
                start: 0,
                end: 10
            },
            ner::Entity {
                label: "ORG".to_string(),
                start: 20,
                end: 29
            },
        ];
        assert_eq!(redact(text, &entities), "[PER] works at [ORG]");
    }

    #[test]
    fn redact_with_no_entities_returns_text_unchanged() {
        let text = "nothing to see here";
        assert_eq!(redact(text, &[]), text);
    }

    #[test]
    fn redact_ignores_an_out_of_order_or_invalid_span_defensively() {
        let text = "abcdef";
        // Second span starts before the first one ends -- must not panic
        // or produce a garbled/duplicated slice; the invalid entry is
        // simply skipped rather than trusted.
        let entities = alloc::vec![
            ner::Entity {
                label: "PER".to_string(),
                start: 0,
                end: 4
            },
            ner::Entity {
                label: "ORG".to_string(),
                start: 2,
                end: 6
            },
        ];
        let out = redact(text, &entities);
        assert_eq!(out, "[PER]ef");
    }

    #[test]
    fn redact_text_runs_full_pipeline_without_panicking() {
        // Not asserting real semantic correctness (fixture weights aren't
        // the real model) -- just that the full encode/forward/decode/
        // redact pipeline completes without panicking on a realistic
        // sentence. Real redaction quality is verified separately against
        // the published `full-model` artifact (see the publish PR).
        let m = model();
        let _out = redact_text(&m, "Elon Musk founded SpaceX in California.");
    }

    #[test]
    fn redact_text_empty_input_returns_empty() {
        let m = model();
        assert_eq!(redact_text(&m, ""), "");
    }

    #[test]
    fn redact_text_is_deterministic() {
        let m = model();
        let a = redact_text(&m, "hello world");
        let b = redact_text(&m, "hello world");
        assert_eq!(a, b);
    }

    #[test]
    fn run_request_missing_text_field_defaults_to_empty() {
        let m = model();
        let input = object(alloc::vec![]);
        let out = run_request(&m, input);
        assert_eq!(out.get("redacted_text").and_then(Value::as_str), Some(""));
    }

    #[test]
    fn run_request_produces_well_formed_output_shape() {
        let m = model();
        let input = object(alloc::vec![(
            "text",
            Value::String("hello world".to_string())
        )]);
        let out = run_request(&m, input);
        assert!(out.get("redacted_text").and_then(Value::as_str).is_some());
    }
}
