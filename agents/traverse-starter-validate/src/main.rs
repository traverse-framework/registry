//! Real, deterministic implementation of the `traverse-starter.validate`
//! capability. Implements exactly the two rules the published contract's
//! `description` already documents (empty/whitespace note is invalid; a note
//! over 2000 characters is flagged) -- this replaces the 1.0.1 fixed-output
//! fixture that always returned `{"valid": true, "issues": []}` regardless
//! of input. See registry#79, docs/decision-log.md.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use wasi_agent_runtime::{array_of_strings, object, Value};

const MAX_NOTE_LENGTH: usize = 2000;

fn validate(input: Value) -> Value {
    let note = input.get("note").and_then(Value::as_str).unwrap_or("");
    let mut issues: Vec<String> = Vec::new();

    if note.trim().is_empty() {
        issues.push(String::from("note is empty or contains only whitespace"));
    }
    if note.chars().count() > MAX_NOTE_LENGTH {
        issues.push(alloc::format!(
            "note exceeds {MAX_NOTE_LENGTH} characters (actual: {})",
            note.chars().count()
        ));
    }

    let valid = issues.is_empty();
    object(alloc::vec![
        ("valid", Value::Bool(valid)),
        ("issues", array_of_strings(&issues)),
    ])
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    wasi_agent_runtime::run_agent(validate);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(note: &str) -> Value {
        validate(object(alloc::vec![("note", Value::String(String::from(note)))]))
    }

    #[test]
    fn empty_note_is_invalid() {
        let out = run("");
        assert_eq!(out.get("valid").unwrap().as_bool().unwrap(), false);
        assert_eq!(out.get("issues").unwrap().string_array().len(), 1);
    }

    #[test]
    fn whitespace_only_note_is_invalid() {
        let out = run("   \n\t  ");
        assert_eq!(out.get("valid").unwrap().as_bool().unwrap(), false);
    }

    #[test]
    fn normal_note_is_valid() {
        let out = run("Buy milk tomorrow.");
        assert_eq!(out.get("valid").unwrap().as_bool().unwrap(), true);
        assert!(out.get("issues").unwrap().string_array().is_empty());
    }

    #[test]
    fn overlong_note_is_flagged() {
        let long_note: String = core::iter::repeat('a').take(2001).collect();
        let out = run(&long_note);
        assert_eq!(out.get("valid").unwrap().as_bool().unwrap(), false);
        let issues = out.get("issues").unwrap().string_array();
        assert!(issues.iter().any(|i| i.contains("2000")));
    }

    #[test]
    fn output_genuinely_differs_across_distinct_inputs() {
        let valid_out = run("a normal note");
        let invalid_out = run("");
        assert_ne!(
            valid_out.get("valid").unwrap().as_bool(),
            invalid_out.get("valid").unwrap().as_bool(),
            "output must depend on input, not be fixed"
        );
    }
}
