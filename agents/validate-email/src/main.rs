//! Real, deterministic implementation of the `validation.validate-email`
//! capability. Structural email-shape validation only -- no DNS/MX/SMTP
//! verification (not possible from inside the WASM sandbox, and disclosed
//! as such in contract.json). Built via traverse-capability-author, the
//! first end-to-end use of that skill.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::string::String;
use wasi_agent_runtime::{object, Value};

const DEFAULT_MAX_LENGTH: usize = 254;

fn validate_email(email: &str, allow_plus_addressing: bool, max_length: usize) -> Option<&'static str> {
    if email.is_empty() {
        return Some("empty");
    }
    if email.chars().count() > max_length {
        return Some("exceeds_max_length");
    }
    if email.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Some("invalid_character");
    }

    let at_count = email.chars().filter(|&c| c == '@').count();
    if at_count == 0 {
        return Some("missing_at_sign");
    }
    if at_count > 1 {
        return Some("multiple_at_signs");
    }

    let mut parts = email.split('@');
    let local = parts.next().unwrap_or("");
    let domain = parts.next().unwrap_or("");

    if local.is_empty() {
        return Some("empty_local_part");
    }
    if domain.is_empty() {
        return Some("empty_domain");
    }
    if local.starts_with('.') {
        return Some("leading_dot_in_local_part");
    }
    if domain.contains("..") {
        return Some("consecutive_dots_in_domain");
    }
    if !domain.contains('.') {
        return Some("domain_missing_dot");
    }
    if !allow_plus_addressing && local.contains('+') {
        return Some("plus_addressing_disabled");
    }
    let local_ok = local
        .chars()
        .all(|c| c.is_alphanumeric() || c == '.' || c == '+' || c == '_' || c == '-');
    if !local_ok {
        return Some("invalid_character");
    }
    let domain_ok = domain.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '-');
    if !domain_ok {
        return Some("invalid_character");
    }

    None
}

fn handle(input: Value) -> Value {
    let email = input.get("email").and_then(Value::as_str).unwrap_or("");
    let allow_plus_addressing = input
        .get("allow_plus_addressing")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let max_length = input
        .get("max_length")
        .and_then(Value::as_f64)
        .map(|n| n as usize)
        .unwrap_or(DEFAULT_MAX_LENGTH);

    match validate_email(email, allow_plus_addressing, max_length) {
        None => object(alloc::vec![
            ("valid", Value::Bool(true)),
            ("reason", Value::Null),
        ]),
        Some(reason) => object(alloc::vec![
            ("valid", Value::Bool(false)),
            ("reason", Value::String(String::from(reason))),
        ]),
    }
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    wasi_agent_runtime::run_agent(handle);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(email: &str) -> Option<&'static str> {
        validate_email(email, true, DEFAULT_MAX_LENGTH)
    }

    #[test]
    fn happy_paths() {
        assert_eq!(check("alice@example.com"), None);
        assert_eq!(check("first.last@sub.example.co.uk"), None);
        assert_eq!(check("alice+newsletter@example.com"), None);
    }

    #[test]
    fn unhappy_paths_from_spec_table() {
        assert_eq!(check(""), Some("empty"));
        assert_eq!(check("alice.example.com"), Some("missing_at_sign"));
        assert_eq!(check("alice@@example.com"), Some("multiple_at_signs"));
        assert_eq!(check("@example.com"), Some("empty_local_part"));
        assert_eq!(check("alice@"), Some("empty_domain"));
        assert_eq!(check("alice@localhost"), Some("domain_missing_dot"));
        assert_eq!(check("alice@example..com"), Some("consecutive_dots_in_domain"));
        assert_eq!(check(".alice@example.com"), Some("leading_dot_in_local_part"));
        assert_eq!(check("alice @example.com"), Some("invalid_character"));
    }

    #[test]
    fn plus_addressing_disabled_config() {
        assert_eq!(
            validate_email("alice+x@example.com", false, DEFAULT_MAX_LENGTH),
            Some("plus_addressing_disabled")
        );
        assert_eq!(
            validate_email("alice+x@example.com", true, DEFAULT_MAX_LENGTH),
            None
        );
    }

    #[test]
    fn max_length_config() {
        let long_local: String = core::iter::repeat('a').take(300).collect();
        let long_email = alloc::format!("{long_local}@example.com");
        assert_eq!(check(&long_email), Some("exceeds_max_length"));
        // A stricter caller-supplied max_length catches a shorter address too.
        assert_eq!(
            validate_email("alice@example.com", true, 5),
            Some("exceeds_max_length")
        );
    }

    #[test]
    fn output_genuinely_differs_across_distinct_inputs() {
        let out_a = handle(object(alloc::vec![("email", Value::String(String::from("alice@example.com")))]));
        let out_b = handle(object(alloc::vec![("email", Value::String(String::from("not-an-email")))]));
        assert_ne!(
            out_a.get("valid").unwrap().as_bool(),
            out_b.get("valid").unwrap().as_bool(),
            "output must depend on input, not be fixed"
        );
    }
}
