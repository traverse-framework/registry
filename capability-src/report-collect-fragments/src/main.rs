//! Real, deterministic implementation of the `report.collect-fragments` capability.
//!
//! First node of the `report.*` chain (ported from the UMA chapter-13
//! `DataProviderLocal`, re-scoped from a canned-fixture provider to a real
//! input normalizer). Takes the caller's raw `fragments` and returns a
//! cleaned `source_fragments` list: each fragment trimmed, empty ones
//! dropped, case-insensitive exact duplicates dropped (first occurrence
//! kept), original order preserved, over-long fragments clipped on a
//! `char` boundary. No model, network, randomness, or host state
//! dependency -- identical input always produces identical output.
//!
//! Built on `wasi-capability-runtime` (`#![no_std]`, hand-rolled JSON,
//! only `fd_read`/`fd_write`/`proc_exit` imports) so it is both genuinely
//! input-dependent and executable by Traverse's runtime ABI whitelist.
//! See traverse-framework/registry#427, #426, docs/decision-log.md.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use wasi_capability_runtime::{array_of_strings, object, Value};

/// Hard guard against pathological input: a fragment longer than this many
/// characters is clipped (always on a `char` boundary). UMA-style source
/// fragments are a sentence or two; this only ever fires on abuse.
const MAX_FRAGMENT_CHARS: usize = 2000;

/// Clip `s` to at most `max` characters, always on a `char` boundary
/// (byte slicing could split a multi-byte UTF-8 sequence).
fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

fn collect_fragments(input: Value) -> Value {
    let raw = input
        .get("fragments")
        .map(Value::string_array)
        .unwrap_or_default();
    let input_count = raw.len();

    let mut kept: Vec<String> = Vec::new();
    let mut seen_lower: Vec<String> = Vec::new();

    for fragment in &raw {
        let trimmed = fragment.trim();
        if trimmed.is_empty() {
            continue;
        }
        let clipped = clip(trimmed, MAX_FRAGMENT_CHARS);
        let lower = clipped.to_lowercase();
        if seen_lower.contains(&lower) {
            continue;
        }
        seen_lower.push(lower);
        kept.push(clipped);
    }

    let dropped = input_count.saturating_sub(kept.len());
    let status = if kept.is_empty() {
        "insufficient_data"
    } else {
        "ok"
    };

    object(alloc::vec![
        ("source_fragments", array_of_strings(&kept)),
        ("dropped_count", Value::Number(dropped as f64)),
        ("status", Value::String(String::from(status))),
    ])
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    wasi_capability_runtime::run_capability(collect_fragments);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(json: &str) -> Value {
        let input = wasi_capability_runtime::parse_json(json).expect("input must parse");
        collect_fragments(input)
    }

    fn fragments_of(output: &Value) -> Vec<String> {
        output.get("source_fragments").unwrap().string_array()
    }

    #[test]
    fn clip_leaves_short_strings_untouched() {
        assert_eq!(clip("hello", MAX_FRAGMENT_CHARS), "hello");
        assert_eq!(clip("", 10), "");
    }

    #[test]
    fn clip_truncates_on_a_char_boundary() {
        // 3 characters, one of them multi-byte -- clipping to 2 must not panic
        // and must keep whole chars.
        assert_eq!(clip("aé9", 2), "aé");
        let long = "x".repeat(5000);
        assert_eq!(clip(&long, MAX_FRAGMENT_CHARS).chars().count(), MAX_FRAGMENT_CHARS);
    }

    #[test]
    fn keeps_distinct_fragments_in_order_with_nothing_dropped() {
        let out = call(r#"{"fragments": ["beta", "alpha", "gamma"]}"#);
        assert_eq!(fragments_of(&out), alloc::vec![
            String::from("beta"),
            String::from("alpha"),
            String::from("gamma"),
        ]);
        assert_eq!(out.get("dropped_count").unwrap().as_f64().unwrap(), 0.0);
        assert_eq!(out.get("status").unwrap().as_str().unwrap(), "ok");
    }

    #[test]
    fn trims_surrounding_whitespace() {
        let out = call(r#"{"fragments": ["  spaced out  ", "\ttabbed\n"]}"#);
        assert_eq!(fragments_of(&out), alloc::vec![
            String::from("spaced out"),
            String::from("tabbed"),
        ]);
    }

    #[test]
    fn drops_empty_and_whitespace_only_fragments_and_counts_them() {
        let out = call(r#"{"fragments": ["real", "", "   ", "\t\n"]}"#);
        assert_eq!(fragments_of(&out), alloc::vec![String::from("real")]);
        assert_eq!(out.get("dropped_count").unwrap().as_f64().unwrap(), 3.0);
        assert_eq!(out.get("status").unwrap().as_str().unwrap(), "ok");
    }

    #[test]
    fn drops_case_insensitive_exact_duplicates_keeping_first() {
        let out = call(r#"{"fragments": ["Edge cache is warm", "edge cache is warm", "EDGE CACHE IS WARM"]}"#);
        assert_eq!(fragments_of(&out), alloc::vec![String::from("Edge cache is warm")]);
        assert_eq!(out.get("dropped_count").unwrap().as_f64().unwrap(), 2.0);
    }

    #[test]
    fn clips_an_over_long_fragment_before_dedupe() {
        let a = "a".repeat(2500);
        let b = "a".repeat(3000);
        // Both clip to 2000 'a's -> the second is a duplicate of the first.
        let json = alloc::format!(r#"{{"fragments": ["{a}", "{b}"]}}"#);
        let out = collect_fragments(wasi_capability_runtime::parse_json(&json).unwrap());
        let frags = fragments_of(&out);
        assert_eq!(frags.len(), 1);
        assert_eq!(frags[0].chars().count(), MAX_FRAGMENT_CHARS);
        assert_eq!(out.get("dropped_count").unwrap().as_f64().unwrap(), 1.0);
    }

    #[test]
    fn all_fragments_empty_yields_insufficient_data() {
        let out = call(r#"{"fragments": ["", "  ", "\n"]}"#);
        assert!(fragments_of(&out).is_empty());
        assert_eq!(out.get("dropped_count").unwrap().as_f64().unwrap(), 3.0);
        assert_eq!(out.get("status").unwrap().as_str().unwrap(), "insufficient_data");
    }

    #[test]
    fn missing_fragments_key_yields_insufficient_data() {
        let out = call(r#"{}"#);
        assert!(fragments_of(&out).is_empty());
        assert_eq!(out.get("dropped_count").unwrap().as_f64().unwrap(), 0.0);
        assert_eq!(out.get("status").unwrap().as_str().unwrap(), "insufficient_data");
    }

    #[test]
    fn output_is_valid_json_and_input_dependent_end_to_end() {
        let out_a = call(r#"{"fragments": ["Browser telemetry shows strong adoption."]}"#);
        let out_b = call(r#"{"fragments": ["An edge cache exposes regional rollout data."]}"#);
        let written_a = wasi_capability_runtime::write_json(&out_a);
        let reparsed = wasi_capability_runtime::parse_json(&written_a).expect("round trips");
        assert_eq!(reparsed.get("status").unwrap().as_str().unwrap(), "ok");
        assert_ne!(fragments_of(&out_a), fragments_of(&out_b), "output must depend on input");
    }

    #[test]
    fn determinism_same_input_same_output() {
        let json = r#"{"fragments": ["cloud record confirms handoff time dropped", "cloud record confirms handoff time dropped", "  "]}"#;
        assert_eq!(
            wasi_capability_runtime::write_json(&call(json)),
            wasi_capability_runtime::write_json(&call(json)),
        );
    }
}
