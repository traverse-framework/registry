//! Real, deterministic implementation of the `traverse-starter.process`
//! capability: classify a note's type by keyword, derive a title from its
//! first line/sentence, tag it, and suggest a next action -- all genuinely
//! read from the input `note`, not a fixed response. See registry#79,
//! docs/decision-log.md.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use wasi_capability_runtime::{array_of_strings, object, Value};

const MAX_TITLE_LENGTH: usize = 60;

fn classify_note_type(lower: &str) -> &'static str {
    if lower.contains("todo") || lower.contains("to-do") || lower.contains("action item") || lower.contains("task") {
        "todo"
    } else if lower.contains("idea") || lower.contains("concept") || lower.contains("what if") {
        "idea"
    } else if lower.contains("meeting") || lower.contains("call with") || lower.contains("sync") {
        "meeting"
    } else if lower.contains('?') || lower.contains("question") {
        "question"
    } else {
        "general"
    }
}

fn derive_title(note: &str) -> String {
    let first_line = note
        .split(['\n', '.'])
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    if first_line.is_empty() {
        return String::from("Untitled note");
    }
    let char_count = first_line.chars().count();
    if char_count <= MAX_TITLE_LENGTH {
        String::from(first_line)
    } else {
        let truncated: String = first_line.chars().take(MAX_TITLE_LENGTH).collect();
        alloc::format!("{truncated}...")
    }
}

fn derive_tags(lower: &str, note_type: &str) -> Vec<String> {
    let mut tags = alloc::vec![String::from(note_type)];
    let keyword_tags: &[(&str, &str)] = &[
        ("urgent", "urgent"),
        ("asap", "urgent"),
        ("blocked", "blocked"),
        ("review", "needs-review"),
        ("follow up", "follow-up"),
        ("follow-up", "follow-up"),
    ];
    for (needle, tag) in keyword_tags {
        if lower.contains(needle) && !tags.iter().any(|t| t == tag) {
            tags.push(String::from(*tag));
        }
    }
    tags
}

fn suggested_next_action(note_type: &str) -> &'static str {
    match note_type {
        "todo" => "Add to the task list and assign an owner.",
        "idea" => "Capture in the backlog for later review.",
        "meeting" => "Share notes with attendees and confirm action items.",
        "question" => "Route to the relevant owner for an answer.",
        _ => "Triage and categorize.",
    }
}

fn derive_status(lower: &str) -> &'static str {
    if lower.contains("done") || lower.contains("completed") || lower.contains("resolved") {
        "completed"
    } else if lower.contains("urgent") || lower.contains("asap") {
        "urgent"
    } else {
        "new"
    }
}

fn process(input: Value) -> Value {
    let note = input.get("note").and_then(Value::as_str).unwrap_or("");
    let lower = note.to_lowercase();

    let note_type = classify_note_type(&lower);
    let title = derive_title(note);
    let tags = derive_tags(&lower, note_type);
    let next_action = suggested_next_action(note_type);
    let status = derive_status(&lower);

    object(alloc::vec![
        ("title", Value::String(title)),
        ("tags", array_of_strings(&tags)),
        ("noteType", Value::String(String::from(note_type))),
        ("suggestedNextAction", Value::String(String::from(next_action))),
        ("status", Value::String(String::from(status))),
    ])
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    wasi_capability_runtime::run_capability(process);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(note: &str) -> Value {
        process(object(alloc::vec![("note", Value::String(String::from(note)))]))
    }

    #[test]
    fn classifies_a_todo_note() {
        let out = run("TODO: call the vendor about the invoice.");
        assert_eq!(out.get("noteType").unwrap().as_str().unwrap(), "todo");
        assert!(out.get("tags").unwrap().string_array().contains(&String::from("todo")));
    }

    #[test]
    fn classifies_a_question_note() {
        let out = run("Should we renew the contract early?");
        assert_eq!(out.get("noteType").unwrap().as_str().unwrap(), "question");
    }

    #[test]
    fn derives_title_from_first_line() {
        let out = run("Renew the office lease\nLandlord wants an answer by Friday.");
        assert_eq!(out.get("title").unwrap().as_str().unwrap(), "Renew the office lease");
    }

    #[test]
    fn empty_note_gets_untitled_title() {
        let out = run("");
        assert_eq!(out.get("title").unwrap().as_str().unwrap(), "Untitled note");
        assert_eq!(out.get("noteType").unwrap().as_str().unwrap(), "general");
    }

    #[test]
    fn urgent_keyword_drives_status_and_tag() {
        let out = run("URGENT: fix the broken deploy ASAP.");
        assert_eq!(out.get("status").unwrap().as_str().unwrap(), "urgent");
        assert!(out.get("tags").unwrap().string_array().contains(&String::from("urgent")));
    }

    #[test]
    fn output_genuinely_differs_across_distinct_inputs() {
        let todo_out = run("TODO: ship the release.");
        let idea_out = run("Idea: what if we cached this instead?");
        assert_ne!(
            todo_out.get("noteType").unwrap().as_str(),
            idea_out.get("noteType").unwrap().as_str(),
            "output must depend on input, not be fixed"
        );
    }
}
