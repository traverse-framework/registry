//! Real, deterministic implementation of `report.enrich-insights`.
//!
//! Second node of the `report.*` chain (registry#428 / epic #426). Takes
//! normalized `source_fragments` and returns `structured_facts`: one
//! `Fact: <signal> — <tidied>` line per non-empty fragment, optionally
//! appending the first `$`-prefixed or bare number token, then stable-sorted
//! by `(signal rank, first-seen index)`. Keyword signal classes are
//! `browser` / `edge` / `cloud` / `general`. No model, network, randomness,
//! or host state.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use wasi_capability_runtime::{array_of_strings, object, Value};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Signal {
    Browser,
    Edge,
    Cloud,
    General,
}

impl Signal {
    fn as_str(self) -> &'static str {
        match self {
            Signal::Browser => "browser",
            Signal::Edge => "edge",
            Signal::Cloud => "cloud",
            Signal::General => "general",
        }
    }

    /// Lower rank sorts first. Ties broken by first-seen index.
    fn rank(self) -> u8 {
        match self {
            Signal::Browser => 0,
            Signal::Edge => 1,
            Signal::Cloud => 2,
            Signal::General => 3,
        }
    }
}

fn classify_signal(fragment: &str) -> Signal {
    let lower = fragment.to_lowercase();
    // First matching class wins when multiple keywords appear.
    if lower.contains("browser") {
        Signal::Browser
    } else if lower.contains("edge") {
        Signal::Edge
    } else if lower.contains("cloud") {
        Signal::Cloud
    } else {
        Signal::General
    }
}

/// Collapse runs of whitespace to a single space and trim ends.
fn tidy(fragment: &str) -> String {
    let mut out = String::new();
    let mut prev_space = false;
    for c in fragment.chars() {
        if c.is_whitespace() {
            if !out.is_empty() && !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// First `$`-prefixed token (e.g. `$12.50`) or bare number token (e.g. `42`,
/// `3.14`). Tokens are whitespace-split; the first match wins.
fn first_number_token(fragment: &str) -> Option<String> {
    for token in fragment.split_whitespace() {
        if let Some(rest) = token.strip_prefix('$') {
            if is_number_token(rest) {
                return Some(String::from(token));
            }
            continue;
        }
        if is_number_token(token) {
            return Some(String::from(token));
        }
    }
    None
}

fn is_number_token(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let mut saw_digit = false;
    let mut saw_dot = false;
    for (i, c) in token.chars().enumerate() {
        if c.is_ascii_digit() {
            saw_digit = true;
            continue;
        }
        if c == '.' && !saw_dot {
            saw_dot = true;
            continue;
        }
        if c == '-' && i == 0 {
            continue;
        }
        return false;
    }
    saw_digit
}

fn build_fact_line(signal: Signal, tidied: &str, number: Option<&str>) -> String {
    match number {
        Some(n) => format!("Fact: {} — {} ({})", signal.as_str(), tidied, n),
        None => format!("Fact: {} — {}", signal.as_str(), tidied),
    }
}

fn enrich_insights(input: Value) -> Value {
    let raw = input
        .get("source_fragments")
        .map(Value::string_array)
        .unwrap_or_default();

    struct Row {
        signal: Signal,
        index: usize,
        line: String,
    }

    let mut rows: Vec<Row> = Vec::new();
    for (index, fragment) in raw.iter().enumerate() {
        let tidied = tidy(fragment);
        if tidied.is_empty() {
            continue;
        }
        let signal = classify_signal(&tidied);
        let number = first_number_token(&tidied);
        let line = build_fact_line(signal, &tidied, number.as_deref());
        rows.push(Row {
            signal,
            index,
            line,
        });
    }

    rows.sort_by(|a, b| {
        a.signal
            .rank()
            .cmp(&b.signal.rank())
            .then_with(|| a.index.cmp(&b.index))
    });

    let facts: Vec<String> = rows.into_iter().map(|r| r.line).collect();
    object(alloc::vec![("structured_facts", array_of_strings(&facts))])
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    wasi_capability_runtime::run_capability(enrich_insights);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(json: &str) -> Value {
        let input = wasi_capability_runtime::parse_json(json).expect("input must parse");
        enrich_insights(input)
    }

    fn facts_of(output: &Value) -> Vec<String> {
        output.get("structured_facts").unwrap().string_array()
    }

    #[test]
    fn classifies_each_signal_class() {
        let out = call(
            r#"{"source_fragments":[
                "Cloud region latency improved",
                "Browser session count rose",
                "Edge cache hit ratio climbed",
                "Ops status is green"
            ]}"#,
        );
        let facts = facts_of(&out);
        assert_eq!(facts.len(), 4);
        // Sorted: browser, edge, cloud, general — not input order.
        assert!(facts[0].starts_with("Fact: browser — "));
        assert!(facts[1].starts_with("Fact: edge — "));
        assert!(facts[2].starts_with("Fact: cloud — "));
        assert!(facts[3].starts_with("Fact: general — "));
    }

    #[test]
    fn first_matching_keyword_wins_when_several_appear() {
        let out = call(r#"{"source_fragments":["browser and edge both mentioned"]}"#);
        let facts = facts_of(&out);
        assert_eq!(facts.len(), 1);
        assert!(facts[0].starts_with("Fact: browser — "));
    }

    #[test]
    fn extracts_dollar_prefixed_number_token() {
        let out = call(r#"{"source_fragments":["Browser spend hit $12.50 yesterday"]}"#);
        let facts = facts_of(&out);
        assert_eq!(
            facts,
            alloc::vec![String::from(
                "Fact: browser — Browser spend hit $12.50 yesterday ($12.50)"
            )]
        );
    }

    #[test]
    fn extracts_bare_number_token_when_no_dollar() {
        let out = call(r#"{"source_fragments":["Edge pods scaled to 42 instances"]}"#);
        let facts = facts_of(&out);
        assert_eq!(
            facts,
            alloc::vec![String::from(
                "Fact: edge — Edge pods scaled to 42 instances (42)"
            )]
        );
    }

    #[test]
    fn omits_number_suffix_when_none_found() {
        let out = call(r#"{"source_fragments":["Cloud handoff improved"]}"#);
        let facts = facts_of(&out);
        assert_eq!(
            facts,
            alloc::vec![String::from("Fact: cloud — Cloud handoff improved")]
        );
    }

    #[test]
    fn tidies_internal_whitespace() {
        assert_eq!(tidy("  a   b\tc  "), "a b c");
        let out = call(r#"{"source_fragments":["  Browser   adoption \t strong  "]}"#);
        assert_eq!(
            facts_of(&out),
            alloc::vec![String::from("Fact: browser — Browser adoption strong")]
        );
    }

    #[test]
    fn skips_empty_fragments_passthrough_count_is_non_empty_only() {
        let out = call(r#"{"source_fragments":["", "  ", "Cloud ok", "\n"]}"#);
        assert_eq!(facts_of(&out).len(), 1);
    }

    #[test]
    fn empty_input_yields_empty_facts() {
        let out = call(r#"{"source_fragments":[]}"#);
        assert!(facts_of(&out).is_empty());
        let missing = call(r#"{}"#);
        assert!(facts_of(&missing).is_empty());
    }

    #[test]
    fn stable_sort_preserves_first_seen_within_same_signal() {
        let out = call(
            r#"{"source_fragments":[
                "Browser alpha note",
                "Cloud first",
                "Browser beta note",
                "Cloud second"
            ]}"#,
        );
        let facts = facts_of(&out);
        assert_eq!(
            facts,
            alloc::vec![
                String::from("Fact: browser — Browser alpha note"),
                String::from("Fact: browser — Browser beta note"),
                String::from("Fact: cloud — Cloud first"),
                String::from("Fact: cloud — Cloud second"),
            ]
        );
    }

    #[test]
    fn determinism_same_input_same_output() {
        let json = r#"{"source_fragments":["Edge cache warm", "browser up", "misc"]}"#;
        assert_eq!(
            wasi_capability_runtime::write_json(&call(json)),
            wasi_capability_runtime::write_json(&call(json)),
        );
    }

    #[test]
    fn output_depends_on_input() {
        let a = facts_of(&call(r#"{"source_fragments":["Browser one"]}"#));
        let b = facts_of(&call(r#"{"source_fragments":["Cloud two"]}"#));
        assert_ne!(a, b);
    }
}
