//! Real, deterministic implementation of the `traverse-starter.summarize`
//! capability: compose `traverse-starter.process`'s output fields into one
//! human-readable summary line and count its words -- genuinely derived
//! from the input fields, not a fixed string. See registry#79,
//! docs/decision-log.md.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::string::String;

use wasi_capability_runtime::{object, Value};

fn join_with_comma(items: &[String]) -> String {
    let mut out = String::new();
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(item);
    }
    out
}

fn summarize(input: Value) -> Value {
    let title = input.get("title").and_then(Value::as_str).unwrap_or("Untitled note");
    let note_type = input.get("noteType").and_then(Value::as_str).unwrap_or("general");
    let status = input.get("status").and_then(Value::as_str).unwrap_or("new");
    let next_action = input
        .get("suggestedNextAction")
        .and_then(Value::as_str)
        .unwrap_or("Triage and categorize.");
    let tags = input
        .get("tags")
        .map(Value::string_array)
        .unwrap_or_default();

    let mut summary = alloc::format!("{title} ({note_type}, {status}). Next: {next_action}");
    if !tags.is_empty() {
        summary.push_str(" [tags: ");
        summary.push_str(&join_with_comma(&tags));
        summary.push(']');
    }

    let word_count = summary.split_whitespace().count();

    object(alloc::vec![
        ("summary", Value::String(summary)),
        ("wordCount", Value::Number(word_count as f64)),
    ])
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    wasi_capability_runtime::run_capability(summarize);
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use wasi_capability_runtime::array_of_strings;

    fn run(title: &str, tags: &[&str], note_type: &str, next_action: &str, status: &str) -> Value {
        let tag_strings: Vec<String> = tags.iter().map(|t| String::from(*t)).collect();
        summarize(object(alloc::vec![
            ("title", Value::String(String::from(title))),
            ("tags", array_of_strings(&tag_strings)),
            ("noteType", Value::String(String::from(note_type))),
            ("suggestedNextAction", Value::String(String::from(next_action))),
            ("status", Value::String(String::from(status))),
        ]))
    }

    #[test]
    fn summary_includes_all_fields() {
        let out = run("Renew the lease", &["todo", "urgent"], "todo", "Assign an owner.", "urgent");
        let summary = out.get("summary").unwrap().as_str().unwrap();
        assert!(summary.contains("Renew the lease"));
        assert!(summary.contains("todo"));
        assert!(summary.contains("urgent"));
        assert!(summary.contains("Assign an owner."));
    }

    #[test]
    fn word_count_matches_summary() {
        let out = run("Short title", &[], "general", "Triage and categorize.", "new");
        let summary = out.get("summary").unwrap().as_str().unwrap();
        let expected_count = summary.split_whitespace().count() as f64;
        assert_eq!(out.get("wordCount").unwrap().as_f64().unwrap(), expected_count);
    }

    #[test]
    fn no_tags_omits_tag_clause() {
        let out = run("A note", &[], "general", "Triage and categorize.", "new");
        let summary = out.get("summary").unwrap().as_str().unwrap();
        assert!(!summary.contains("[tags:"));
    }

    #[test]
    fn output_genuinely_differs_across_distinct_inputs() {
        let out_a = run("Note A", &["todo"], "todo", "Do it.", "new");
        let out_b = run("Note B entirely different and much longer title text", &["idea", "review"], "idea", "Review later.", "new");
        assert_ne!(
            out_a.get("summary").unwrap().as_str(),
            out_b.get("summary").unwrap().as_str(),
            "output must depend on input, not be fixed"
        );
        assert_ne!(out_a.get("wordCount").unwrap().as_f64(), out_b.get("wordCount").unwrap().as_f64());
    }
}
