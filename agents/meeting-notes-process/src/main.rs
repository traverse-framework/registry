//! Real, deterministic implementation of the `meeting-notes.process`
//! capability: scans the transcript line by line for action-item,
//! decision, and follow-up signal keywords, extracting an owner/due date
//! heuristically from surrounding words -- genuinely derived from the
//! input `transcript`, not a fixed response. See registry#79,
//! docs/decision-log.md.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use wasi_agent_runtime::{object, Value};

fn is_capitalized_word(word: &str) -> bool {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) if first.is_uppercase() => chars.all(|c| c.is_lowercase() || c.is_ascii_digit()),
        _ => false,
    }
}

fn leading_name(line: &str) -> Option<String> {
    let first_word = line.split_whitespace().next()?;
    let trimmed = first_word.trim_matches(|c: char| !c.is_alphanumeric());
    if is_capitalized_word(trimmed) {
        Some(String::from(trimmed))
    } else {
        None
    }
}

fn find_due(line: &str) -> Option<String> {
    let words: Vec<&str> = line.split_whitespace().collect();
    for i in 0..words.len() {
        if words[i].eq_ignore_ascii_case("by") && i + 1 < words.len() {
            let due = words[i + 1].trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
            if !due.is_empty() {
                return Some(String::from(due));
            }
        }
    }
    None
}

fn optional_string(value: Option<String>) -> Value {
    match value {
        Some(s) => Value::String(s),
        None => Value::Null,
    }
}

fn process(input: Value) -> Value {
    let transcript = input.get("transcript").and_then(Value::as_str).unwrap_or("");

    let mut action_items: Vec<Value> = Vec::new();
    let mut decisions: Vec<Value> = Vec::new();
    let mut follow_ups: Vec<String> = Vec::new();
    let mut line_count = 0usize;

    for raw_line in transcript.split(['\n', '.']) {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        line_count += 1;

        let lower_line: String = line.chars().map(|c| c.to_ascii_lowercase()).collect();
        let is_action =
            lower_line.contains("will ") || lower_line.contains("todo") || lower_line.contains("to-do") || lower_line.contains("action item");
        let is_decision = lower_line.contains("decided") || lower_line.contains("decision") || lower_line.contains("agreed");
        let is_follow_up = lower_line.contains("follow up") || lower_line.contains("follow-up") || lower_line.contains("next step");

        if is_action {
            action_items.push(object(alloc::vec![
                ("task", Value::String(String::from(line))),
                ("owner", optional_string(leading_name(line))),
                ("due", optional_string(find_due(line))),
            ]));
        } else if is_decision {
            decisions.push(object(alloc::vec![
                ("text", Value::String(String::from(line))),
                ("made_by", optional_string(leading_name(line))),
            ]));
        } else if is_follow_up {
            follow_ups.push(String::from(line));
        }
    }

    let summary = alloc::format!(
        "{} action item{}, {} decision{}, and {} follow-up{} identified from {} line{} of transcript.",
        action_items.len(),
        if action_items.len() == 1 { "" } else { "s" },
        decisions.len(),
        if decisions.len() == 1 { "" } else { "s" },
        follow_ups.len(),
        if follow_ups.len() == 1 { "" } else { "s" },
        line_count,
        if line_count == 1 { "" } else { "s" },
    );

    object(alloc::vec![
        ("action_items", Value::Array(action_items)),
        ("decisions", Value::Array(decisions)),
        ("follow_ups", Value::Array(follow_ups.into_iter().map(Value::String).collect())),
        ("summary", Value::String(summary)),
    ])
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    wasi_agent_runtime::run_agent(process);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(transcript: &str) -> Value {
        process(object(alloc::vec![(
            "transcript",
            Value::String(String::from(transcript))
        )]))
    }

    #[test]
    fn extracts_an_action_item_with_owner_and_due() {
        let out = run("Bob will send the report by Friday.");
        let items = out.get("action_items").unwrap().as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].get("owner").unwrap().as_str().unwrap(), "Bob");
        assert_eq!(items[0].get("due").unwrap().as_str().unwrap(), "Friday");
    }

    #[test]
    fn extracts_a_decision() {
        let out = run("Alice decided we should ship next week.");
        let decisions = out.get("decisions").unwrap().as_array().unwrap();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].get("made_by").unwrap().as_str().unwrap(), "Alice");
    }

    #[test]
    fn extracts_a_follow_up() {
        let out = run("Follow up with legal about the contract terms.");
        let follow_ups = out.get("follow_ups").unwrap().string_array();
        assert_eq!(follow_ups.len(), 1);
    }

    #[test]
    fn plain_transcript_with_no_signals_yields_empty_arrays() {
        let out = run("We had a nice chat about the weather today");
        assert!(out.get("action_items").unwrap().as_array().unwrap().is_empty());
        assert!(out.get("decisions").unwrap().as_array().unwrap().is_empty());
        assert!(out.get("follow_ups").unwrap().string_array().is_empty());
    }

    #[test]
    fn output_genuinely_differs_across_distinct_inputs() {
        let out_a = run("Bob will send the report by Friday. Alice decided we should ship next week.");
        let out_b = run("We had a nice chat about the weather today");
        assert_ne!(
            out_a.get("action_items").unwrap().as_array().unwrap().len(),
            out_b.get("action_items").unwrap().as_array().unwrap().len(),
            "output must depend on input, not be fixed"
        );
    }
}
