//! Real, deterministic extractive summarizer for `report.summarize`.
//!
//! Third node of the `report.*` chain (registry#429 / epic #426). Scores
//! sentences from `source_fragments` by keyword overlap with
//! `structured_facts`, keeps the top-k in original order, and renders a
//! fixed template. No model, network, randomness, or host state.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use wasi_capability_runtime::{object, Value};

/// Fixed number of sentences retained for the extractive body.
const TOP_K: usize = 3;

fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        current.push(c);
        if c == '.' || c == '!' || c == '?' {
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                out.push(String::from(trimmed));
            }
            current.clear();
        }
    }
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        out.push(String::from(trimmed));
    }
    out
}

fn tokenize_lower(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            current.push(c.to_ascii_lowercase());
        } else if !current.is_empty() {
            tokens.push(current.clone());
            current.clear();
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn score_sentence(sentence: &str, fact_tokens: &[String]) -> usize {
    if fact_tokens.is_empty() {
        return 0;
    }
    let sent_tokens = tokenize_lower(sentence);
    let mut score = 0usize;
    for token in &sent_tokens {
        if fact_tokens.iter().any(|f| f == token) {
            score += 1;
        }
    }
    score
}

fn build_summary(
    source_fragments: &[String],
    structured_facts: &[String],
    project_name: Option<&str>,
) -> String {
    let mut sentences: Vec<(usize, String)> = Vec::new();
    let mut idx = 0usize;
    for fragment in source_fragments {
        for sentence in split_sentences(fragment) {
            sentences.push((idx, sentence));
            idx += 1;
        }
    }

    let mut fact_tokens = Vec::new();
    for fact in structured_facts {
        for token in tokenize_lower(fact) {
            if !fact_tokens.iter().any(|t| t == &token) {
                fact_tokens.push(token);
            }
        }
    }

    let mut ranked: Vec<(usize, usize, String)> = sentences
        .into_iter()
        .map(|(i, s)| (score_sentence(&s, &fact_tokens), i, s))
        .collect();

    // Higher score first; ties keep original order (lower index first).
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    let mut selected: Vec<(usize, String)> = ranked
        .into_iter()
        .take(TOP_K)
        .map(|(_score, i, s)| (i, s))
        .collect();
    selected.sort_by_key(|(i, _)| *i);

    let project = match project_name.map(str::trim).filter(|s| !s.is_empty()) {
        Some(name) => name,
        None => "The project",
    };
    let n = structured_facts.len();
    let insight_word = if n == 1 { "insight" } else { "insights" };
    let header = format!(
        "{project} combines distributed browser, edge, and cloud evidence into a deterministic operational summary with {n} validated {insight_word}."
    );

    if selected.is_empty() {
        return header;
    }

    let mut body = String::new();
    for (_i, sentence) in &selected {
        if !body.is_empty() {
            body.push(' ');
        }
        body.push_str(sentence);
    }
    format!("{header} {body}")
}

fn summarize(input: Value) -> Value {
    let source_fragments = input
        .get("source_fragments")
        .map(Value::string_array)
        .unwrap_or_default();
    let structured_facts = input
        .get("structured_facts")
        .map(Value::string_array)
        .unwrap_or_default();
    let project_name = input.get("project_name").and_then(Value::as_str);

    let summary = build_summary(&source_fragments, &structured_facts, project_name);
    object(alloc::vec![("summary", Value::String(summary))])
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    wasi_capability_runtime::run_capability(summarize);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(json: &str) -> Value {
        let input = wasi_capability_runtime::parse_json(json).expect("parse");
        summarize(input)
    }

    fn summary_of(out: &Value) -> &str {
        out.get("summary").unwrap().as_str().unwrap()
    }

    #[test]
    fn defaults_project_name_and_pluralizes_insights() {
        let out = call(
            r#"{"source_fragments":["Browser adoption rose. Edge cache is warm."],"structured_facts":["Fact: browser — Browser adoption rose","Fact: edge — Edge cache is warm"]}"#,
        );
        let s = summary_of(&out);
        assert!(s.starts_with(
            "The project combines distributed browser, edge, and cloud evidence into a deterministic operational summary with 2 validated insights."
        ));
    }

    #[test]
    fn uses_project_name_and_singular_insight() {
        let out = call(
            r#"{"source_fragments":["Cloud latency improved."],"structured_facts":["Fact: cloud — Cloud latency improved"],"project_name":"Acme Rollout"}"#,
        );
        let s = summary_of(&out);
        assert!(s.starts_with(
            "Acme Rollout combines distributed browser, edge, and cloud evidence into a deterministic operational summary with 1 validated insight."
        ));
    }

    #[test]
    fn zero_facts_still_renders_header() {
        let out = call(r#"{"source_fragments":["Unrelated chatter here."],"structured_facts":[]}"#);
        let s = summary_of(&out);
        assert!(s.contains("with 0 validated insights."));
    }

    #[test]
    fn top_k_larger_than_sentence_count_keeps_all_in_order() {
        let out = call(
            r#"{"source_fragments":["Alpha browser note. Beta edge note."],"structured_facts":["browser","edge"]}"#,
        );
        let s = summary_of(&out);
        let alpha = s.find("Alpha browser note.").unwrap();
        let beta = s.find("Beta edge note.").unwrap();
        assert!(alpha < beta);
    }

    #[test]
    fn prefers_higher_overlap_sentences() {
        let out = call(
            r#"{"source_fragments":["zzz unrelated filler. Browser telemetry rose sharply. more filler words here."],"structured_facts":["Fact: browser — Browser telemetry rose sharply"]}"#,
        );
        let s = summary_of(&out);
        assert!(s.contains("Browser telemetry rose sharply."));
    }

    #[test]
    fn tie_break_keeps_earlier_sentence() {
        // Equal overlap tokens → original order.
        let out = call(
            r#"{"source_fragments":["Alpha browser. Beta browser."],"structured_facts":["browser"]}"#,
        );
        let s = summary_of(&out);
        assert!(s.find("Alpha browser.").unwrap() < s.find("Beta browser.").unwrap());
    }

    #[test]
    fn missing_project_name_key_defaults() {
        let out = call(r#"{"source_fragments":[],"structured_facts":[]}"#);
        assert!(summary_of(&out).starts_with("The project combines"));
    }

    #[test]
    fn empty_project_name_defaults() {
        let out = call(r#"{"source_fragments":[],"structured_facts":[],"project_name":"  "}"#);
        assert!(summary_of(&out).starts_with("The project combines"));
    }

    #[test]
    fn determinism_byte_for_byte() {
        let json = r#"{"source_fragments":["Browser up. Edge warm. Cloud ok."],"structured_facts":["browser","edge","cloud"],"project_name":"Demo"}"#;
        assert_eq!(
            wasi_capability_runtime::write_json(&call(json)),
            wasi_capability_runtime::write_json(&call(json)),
        );
    }

    #[test]
    fn output_depends_on_input() {
        let a = summary_of(&call(
            r#"{"source_fragments":["Browser one."],"structured_facts":["browser"]}"#,
        ))
        .to_string();
        let b = summary_of(&call(
            r#"{"source_fragments":["Cloud two."],"structured_facts":["cloud"]}"#,
        ))
        .to_string();
        assert_ne!(a, b);
    }
}
