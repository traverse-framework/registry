//! Real, deterministic implementation of the
//! `validation.normalize-phone-number` capability. Format/plausibility
//! checking against a disclosed, finite table of country calling codes --
//! no carrier/HLR lookup (not possible from inside the WASM sandbox).

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::string::String;
use wasi_capability_runtime::{object, Value};

fn calling_code_for_country(country: &str) -> Option<&'static str> {
    match country {
        "US" | "CA" => Some("1"),
        "GB" => Some("44"),
        "DE" => Some("49"),
        "FR" => Some("33"),
        "JP" => Some("81"),
        "AU" => Some("61"),
        "IN" => Some("91"),
        _ => None,
    }
}

fn expected_digit_range(calling_code: &str) -> (usize, usize) {
    match calling_code {
        "1" => (10, 10),
        "44" => (10, 10),
        "49" => (10, 11),
        "33" => (9, 9),
        "81" => (9, 10),
        "61" => (9, 9),
        "91" => (10, 10),
        _ => (0, 0),
    }
}

struct NormalizeResult {
    valid: bool,
    e164: Option<String>,
    country_code: Option<String>,
    national_number: Option<String>,
    reason: Option<&'static str>,
}

fn normalize_phone(phone: &str, default_country: &str) -> NormalizeResult {
    let empty_result = |reason: &'static str| NormalizeResult {
        valid: false,
        e164: None,
        country_code: None,
        national_number: None,
        reason: Some(reason),
    };

    if phone.is_empty() {
        return empty_result("empty");
    }

    let has_plus = phone.trim_start().starts_with('+');
    let digits: String = phone.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return empty_result("no_digits_found");
    }

    let (calling_code, national): (Option<String>, String) = if has_plus {
        if digits.len() >= 2 && matches!(&digits[0..2], "44" | "49" | "33" | "81" | "61" | "91") {
            (Some(String::from(&digits[0..2])), String::from(&digits[2..]))
        } else if digits.starts_with('1') {
            (Some(String::from("1")), String::from(&digits[1..]))
        } else {
            (None, String::new())
        }
    } else {
        match calling_code_for_country(default_country) {
            Some(code) => (Some(String::from(code)), digits.clone()),
            None => (None, String::new()),
        }
    };

    let calling_code = match calling_code {
        Some(c) => c,
        None => return empty_result("unsupported_country_code"),
    };

    let (min_len, max_len) = expected_digit_range(&calling_code);
    let n = national.chars().count();
    if n < min_len || n > max_len {
        return NormalizeResult {
            valid: false,
            e164: None,
            country_code: Some(calling_code),
            national_number: Some(national),
            reason: Some("digit_count_implausible"),
        };
    }

    let e164 = alloc::format!("+{calling_code}{national}");
    NormalizeResult {
        valid: true,
        e164: Some(e164),
        country_code: Some(calling_code),
        national_number: Some(national),
        reason: None,
    }
}

fn handle(input: Value) -> Value {
    let phone = input.get("phone").and_then(Value::as_str).unwrap_or("");
    let default_country = input
        .get("default_country")
        .and_then(Value::as_str)
        .unwrap_or("US");
    let result = normalize_phone(phone, default_country);

    object(alloc::vec![
        ("valid", Value::Bool(result.valid)),
        (
            "e164",
            result.e164.map(Value::String).unwrap_or(Value::Null),
        ),
        (
            "country_code",
            result.country_code.map(Value::String).unwrap_or(Value::Null),
        ),
        (
            "national_number",
            result
                .national_number
                .map(Value::String)
                .unwrap_or(Value::Null),
        ),
        ("line_type", Value::String(String::from("unknown"))),
        (
            "reason",
            result
                .reason
                .map(|r| Value::String(String::from(r)))
                .unwrap_or(Value::Null),
        ),
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

    #[test]
    fn happy_path_us_default() {
        let r = normalize_phone("(415) 555-2671", "US");
        assert!(r.valid);
        assert_eq!(r.e164.as_deref(), Some("+14155552671"));
        assert_eq!(r.country_code.as_deref(), Some("1"));
    }

    #[test]
    fn happy_path_explicit_plus_overrides_default_country() {
        let r = normalize_phone("+44 20 7946 0958", "US");
        assert!(r.valid);
        assert_eq!(r.e164.as_deref(), Some("+442079460958"));
    }

    #[test]
    fn unhappy_paths_from_spec_table() {
        assert_eq!(normalize_phone("", "US").reason, Some("empty"));
        assert_eq!(
            normalize_phone("555-2671", "US").reason,
            Some("digit_count_implausible")
        );
        assert_eq!(
            normalize_phone("+999 555 1234", "US").reason,
            Some("unsupported_country_code")
        );
        assert_eq!(
            normalize_phone("call me maybe", "US").reason,
            Some("no_digits_found")
        );
        assert_eq!(
            normalize_phone("415 555", "ZZ").reason,
            Some("unsupported_country_code")
        );
    }

    #[test]
    fn default_country_config_changes_result() {
        // 9-digit national significant number (no domestic trunk prefix --
        // see the contract's disclosed limitation on that) valid for GB
        // (expects 10) under one default_country and not the other, since
        // digit-count plausibility is checked against the resolved
        // calling code's expected range either way.
        let fr = normalize_phone("20 7946 095", "FR"); // 9 digits, FR expects 9
        let gb = normalize_phone("20 7946 095", "GB"); // 9 digits, GB expects 10
        assert_ne!(fr.valid, gb.valid, "same digits, different default_country must differ");
        assert!(fr.valid);
        assert_eq!(fr.e164.as_deref(), Some("+33207946095"));
    }

    #[test]
    fn domestic_trunk_prefix_is_a_disclosed_limitation_not_silently_handled() {
        // "020 7946 0958" is how a UK number is normally written for
        // domestic dialing; the leading trunk 0 is not part of the E.164
        // national number and this capability does not strip it -- caller
        // must supply the national-significant-number form (no leading 0)
        // when relying on default_country, or the full +44 form.
        let r = normalize_phone("020 7946 0958", "GB");
        assert_eq!(r.reason, Some("digit_count_implausible"));
    }

    #[test]
    fn line_type_is_always_unknown() {
        let r = handle(object(alloc::vec![(
            "phone",
            Value::String(String::from("+14155552671")),
        )]));
        assert_eq!(r.get("line_type").unwrap().as_str(), Some("unknown"));
    }

    #[test]
    fn output_genuinely_differs_across_distinct_inputs() {
        let out_a = handle(object(alloc::vec![(
            "phone",
            Value::String(String::from("+14155552671")),
        )]));
        let out_b = handle(object(alloc::vec![(
            "phone",
            Value::String(String::from("not a phone")),
        )]));
        assert_ne!(
            out_a.get("valid").unwrap().as_bool(),
            out_b.get("valid").unwrap().as_bool(),
            "output must depend on input, not be fixed"
        );
    }
}
