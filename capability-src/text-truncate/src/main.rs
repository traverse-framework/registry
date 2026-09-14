//! Deterministic implementation of `text.truncate`.
//! Length is measured in Rust `char` values (Unicode scalar values), not bytes.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::string::String;
use wasi_capability_runtime::{object, Value};

fn non_negative_integer(value: Option<&Value>) -> Option<usize> {
    let number = value?.as_f64()?;
    if !number.is_finite() || number < 0.0 || number > usize::MAX as f64 {
        return None;
    }
    let integer = number as usize;
    (number == integer as f64).then_some(integer)
}

fn take_chars(value: &str, count: usize) -> String {
    value.chars().take(count).collect()
}

fn truncate_text(text: &str, max_length: usize, ellipsis: &str) -> (String, bool) {
    if text.chars().count() <= max_length {
        return (String::from(text), false);
    }
    if max_length == 0 {
        return (String::new(), true);
    }

    let ellipsis_length = ellipsis.chars().count();
    if ellipsis_length >= max_length {
        return (take_chars(ellipsis, max_length), true);
    }

    let prefix_length = max_length - ellipsis_length;
    let mut output = take_chars(text, prefix_length);
    output.push_str(ellipsis);
    (output, true)
}

fn handle(input: Value) -> Value {
    let text = input.get("text").and_then(Value::as_str).unwrap_or("");
    let max_length = non_negative_integer(input.get("max_length")).unwrap_or(0);
    let ellipsis = input.get("ellipsis").and_then(Value::as_str).unwrap_or("…");
    let (text, truncated) = truncate_text(text, max_length, ellipsis);

    object(alloc::vec![
        ("text", Value::String(text)),
        ("truncated", Value::Bool(truncated)),
    ])
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    wasi_capability_runtime::run_capability(handle);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(text: &str, max_length: f64, ellipsis: Option<&str>) -> Value {
        let mut fields = alloc::vec![
            ("text", Value::String(String::from(text))),
            ("max_length", Value::Number(max_length)),
        ];
        if let Some(value) = ellipsis {
            fields.push(("ellipsis", Value::String(String::from(value))));
        }
        object(fields)
    }

    fn output_text(value: &Value) -> Option<&str> {
        value.get("text").and_then(Value::as_str)
    }

    fn output_truncated(value: &Value) -> Option<bool> {
        value.get("truncated").and_then(Value::as_bool)
    }

    #[test]
    fn returns_short_text_unchanged() {
        let output = handle(request("hello", 5.0, None));
        assert_eq!(output_text(&output), Some("hello"));
        assert_eq!(output_truncated(&output), Some(false));
    }

    #[test]
    fn uses_default_ellipsis_within_total_budget() {
        let output = handle(request("hello world", 6.0, None));
        assert_eq!(output_text(&output), Some("hello…"));
        assert_eq!(output_truncated(&output), Some(true));
    }

    #[test]
    fn zero_length_returns_empty_text() {
        let output = handle(request("hello", 0.0, None));
        assert_eq!(output_text(&output), Some(""));
        assert_eq!(output_truncated(&output), Some(true));
    }

    #[test]
    fn custom_ellipsis_is_counted_in_limit() {
        let output = handle(request("abcdefgh", 5.0, Some("...")));
        assert_eq!(output_text(&output), Some("ab..."));
    }

    #[test]
    fn ellipsis_is_clipped_when_it_exceeds_budget() {
        let output = handle(request("abcdef", 2.0, Some("...")));
        assert_eq!(output_text(&output), Some(".."));
    }

    #[test]
    fn empty_ellipsis_uses_entire_budget_for_text() {
        let output = handle(request("abcdef", 3.0, Some("")));
        assert_eq!(output_text(&output), Some("abc"));
    }

    #[test]
    fn counts_unicode_scalars_not_utf8_bytes() {
        let output = handle(request("é😊abc", 3.0, None));
        assert_eq!(output_text(&output), Some("é😊…"));
    }

    #[test]
    fn invalid_lengths_are_rejected_by_parser() {
        assert_eq!(non_negative_integer(None), None);
        assert_eq!(
            non_negative_integer(Some(&Value::String(String::from("3")))),
            None
        );
        assert_eq!(non_negative_integer(Some(&Value::Number(-1.0))), None);
        assert_eq!(non_negative_integer(Some(&Value::Number(1.5))), None);
        assert_eq!(
            non_negative_integer(Some(&Value::Number(f64::INFINITY))),
            None
        );
    }

    #[test]
    fn missing_fields_have_deterministic_fallbacks() {
        let output = handle(object(alloc::vec![]));
        assert_eq!(output_text(&output), Some(""));
        assert_eq!(output_truncated(&output), Some(false));
    }
}
