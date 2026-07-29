//! Real, deterministic implementation of the `validation.validate-luhn`
//! capability. Checksum-format check only -- explicitly not a fraud or
//! authorization signal (see contract.json's description and SPEC.md).

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use wasi_agent_runtime::{object, Value};

fn luhn_check(number: &str, strip_formatting: bool) -> Result<bool, &'static str> {
    if number.is_empty() {
        return Err("empty");
    }

    let filtered: Vec<char> = if strip_formatting {
        number.chars().filter(|c| !c.is_whitespace() && *c != '-').collect()
    } else {
        number.chars().collect()
    };

    if filtered.iter().any(|c| !c.is_ascii_digit()) {
        return Err("non_digit_characters");
    }
    if filtered.len() < 2 {
        return Err("too_short");
    }

    let mut sum: u32 = 0;
    for (i, c) in filtered.iter().rev().enumerate() {
        let mut digit = c.to_digit(10).unwrap_or(0);
        if i % 2 == 1 {
            digit *= 2;
            if digit > 9 {
                digit -= 9;
            }
        }
        sum += digit;
    }

    if sum % 10 == 0 {
        Ok(true)
    } else {
        Err("checksum_failed")
    }
}

fn handle(input: Value) -> Value {
    let number = input.get("number").and_then(Value::as_str).unwrap_or("");
    let strip_formatting = input
        .get("strip_formatting")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    match luhn_check(number, strip_formatting) {
        Ok(valid) => object(alloc::vec![
            ("valid", Value::Bool(valid)),
            ("reason", Value::Null),
        ]),
        Err(reason) => object(alloc::vec![
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

    #[test]
    fn happy_paths_from_spec_table() {
        assert_eq!(luhn_check("4111111111111111", true), Ok(true));
        assert_eq!(luhn_check("4111 1111 1111 1111", true), Ok(true));
    }

    #[test]
    fn unhappy_paths_from_spec_table() {
        assert_eq!(luhn_check("", true), Err("empty"));
        assert_eq!(luhn_check("4111111111111112", true), Err("checksum_failed"));
        assert_eq!(luhn_check("4111-1111-1111-1111", false), Err("non_digit_characters"));
        assert_eq!(luhn_check("abcd1234", true), Err("non_digit_characters"));
        assert_eq!(luhn_check("4", true), Err("too_short"));
    }

    #[test]
    fn strip_formatting_config_changes_result() {
        let with_stripping = luhn_check("4111-1111-1111-1111", true);
        let without_stripping = luhn_check("4111-1111-1111-1111", false);
        assert_ne!(with_stripping, without_stripping, "same input, different strip_formatting must differ");
        assert_eq!(with_stripping, Ok(true));
    }

    #[test]
    fn reason_never_echoes_the_input_number() {
        let out = handle(object(alloc::vec![(
            "number",
            Value::String(String::from("4111111111111112")),
        )]));
        let written = wasi_agent_runtime::write_json(&out);
        assert!(!written.contains("4111111111111112"));
    }

    #[test]
    fn output_genuinely_differs_across_distinct_inputs() {
        let out_a = handle(object(alloc::vec![(
            "number",
            Value::String(String::from("4111111111111111")),
        )]));
        let out_b = handle(object(alloc::vec![(
            "number",
            Value::String(String::from("4111111111111112")),
        )]));
        assert_ne!(
            out_a.get("valid").unwrap().as_bool(),
            out_b.get("valid").unwrap().as_bool(),
            "output must depend on input, not be fixed"
        );
    }
}
