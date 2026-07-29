//! Real, deterministic implementation of the `formatting.format-currency`
//! capability. Formats an amount using a disclosed, finite table of
//! currency decimal-place rules and locale symbol-placement conventions --
//! not a live ISO 4217 registry or full ICU/CLDR data. Unrecognized
//! currency/locale falls back to a generic format with `supported: false`
//! rather than guessing.
//!
//! Number formatting avoids `f64::round`/`abs`/`fract` throughout: on
//! `wasm32-unknown-unknown` those can require libm symbols not linked into
//! a `no_std` binary. Everything here uses casts and integer arithmetic
//! only, the same discipline established in `wasi-agent-runtime`'s own
//! JSON number writer.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::string::String;
use wasi_agent_runtime::{object, Value};

struct CurrencyInfo {
    decimal_places: u32,
    symbol: &'static str,
}

fn currency_info(code: &str) -> Option<CurrencyInfo> {
    match code {
        "USD" => Some(CurrencyInfo { decimal_places: 2, symbol: "$" }),
        "EUR" => Some(CurrencyInfo { decimal_places: 2, symbol: "\u{20ac}" }),
        "GBP" => Some(CurrencyInfo { decimal_places: 2, symbol: "\u{a3}" }),
        "JPY" => Some(CurrencyInfo { decimal_places: 0, symbol: "\u{a5}" }),
        "CAD" => Some(CurrencyInfo { decimal_places: 2, symbol: "$" }),
        "AUD" => Some(CurrencyInfo { decimal_places: 2, symbol: "$" }),
        "INR" => Some(CurrencyInfo { decimal_places: 2, symbol: "\u{20b9}" }),
        "BHD" => Some(CurrencyInfo { decimal_places: 3, symbol: "BHD" }),
        _ => None,
    }
}

struct LocaleInfo {
    symbol_before: bool,
    space_between: bool,
    thousands_sep: char,
    decimal_sep: char,
}

fn locale_info(locale: &str) -> Option<LocaleInfo> {
    match locale {
        "en-US" => Some(LocaleInfo { symbol_before: true, space_between: false, thousands_sep: ',', decimal_sep: '.' }),
        "en-GB" => Some(LocaleInfo { symbol_before: true, space_between: false, thousands_sep: ',', decimal_sep: '.' }),
        "de-DE" => Some(LocaleInfo { symbol_before: false, space_between: true, thousands_sep: '.', decimal_sep: ',' }),
        "fr-FR" => Some(LocaleInfo { symbol_before: false, space_between: true, thousands_sep: ' ', decimal_sep: ',' }),
        "ja-JP" => Some(LocaleInfo { symbol_before: true, space_between: false, thousands_sep: ',', decimal_sep: '.' }),
        _ => None,
    }
}

/// Groups an unsigned integer's digits with `sep` every 3 digits from the right.
fn group_digits(mut n: i64, sep: char) -> String {
    if n == 0 {
        return String::from("0");
    }
    let mut digits: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    while n > 0 {
        digits.push((n % 10) as u8);
        n /= 10;
    }
    let mut out = String::new();
    let len = digits.len();
    for (i, d) in digits.iter().rev().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            out.push(sep);
        }
        out.push((b'0' + d) as char);
    }
    out
}

fn pad_left_zero(mut n: i64, width: usize) -> String {
    let mut s = String::new();
    if n == 0 {
        for _ in 0..width {
            s.push('0');
        }
        return s;
    }
    let mut digits: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    while n > 0 {
        digits.push((n % 10) as u8);
        n /= 10;
    }
    while digits.len() < width {
        digits.push(0);
    }
    for d in digits.iter().rev() {
        s.push((b'0' + d) as char);
    }
    s
}

/// Splits `amount` into (is_negative, integer_part, fractional_part) at
/// `decimal_places` digits of precision, rounding half-up via a +0.5
/// offset before a truncating cast (no `.round()`, no libm).
fn split_amount(amount: f64, decimal_places: u32) -> (bool, i64, i64) {
    let negative = amount < 0.0;
    let magnitude = if negative { -amount } else { amount };
    let mut factor = 1i64;
    for _ in 0..decimal_places {
        factor *= 10;
    }
    let scaled = (magnitude * (factor as f64) + 0.5) as i64;
    let integer_part = scaled / factor;
    let fractional_part = scaled % factor;
    (negative, integer_part, fractional_part)
}

fn format_currency(amount: f64, currency_code: &str, locale: &str) -> (String, bool) {
    let currency = currency_info(currency_code);
    let locale_fmt = locale_info(locale);
    let supported = currency.is_some() && locale_fmt.is_some();

    if !supported {
        // Generic, deliberately plain fallback: never mimics a real
        // locale's style for an unrecognized currency/locale combination.
        let decimal_places = currency.as_ref().map(|c| c.decimal_places).unwrap_or(2);
        let (negative, int_part, frac_part) = split_amount(amount, decimal_places);
        let mut out = String::new();
        if negative {
            out.push('-');
        }
        out.push_str(currency_code);
        out.push(' ');
        out.push_str(&group_digits(int_part, ','));
        if decimal_places > 0 {
            out.push('.');
            out.push_str(&pad_left_zero(frac_part, decimal_places as usize));
        }
        return (out, false);
    }

    let currency = currency.unwrap();
    let loc = locale_fmt.unwrap();
    let (negative, int_part, frac_part) = split_amount(amount, currency.decimal_places);

    let mut number = group_digits(int_part, loc.thousands_sep);
    if currency.decimal_places > 0 {
        number.push(loc.decimal_sep);
        number.push_str(&pad_left_zero(frac_part, currency.decimal_places as usize));
    }

    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if loc.symbol_before {
        out.push_str(currency.symbol);
        if loc.space_between {
            out.push(' ');
        }
        out.push_str(&number);
    } else {
        out.push_str(&number);
        if loc.space_between {
            out.push(' ');
        }
        out.push_str(currency.symbol);
    }
    (out, true)
}

fn handle(input: Value) -> Value {
    let amount = input.get("amount").and_then(Value::as_f64).unwrap_or(0.0);
    let currency_code = input.get("currency_code").and_then(Value::as_str).unwrap_or("");
    let locale = input.get("locale").and_then(Value::as_str).unwrap_or("en-US");

    let (formatted, supported) = format_currency(amount, currency_code, locale);

    object(alloc::vec![
        ("formatted", Value::String(formatted)),
        ("supported", Value::Bool(supported)),
    ])
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
        assert_eq!(format_currency(1234.5, "USD", "en-US"), (String::from("$1,234.50"), true));
        assert_eq!(format_currency(1234.5, "EUR", "de-DE"), (String::from("1.234,50 \u{20ac}"), true));
        assert_eq!(format_currency(1500.0, "JPY", "ja-JP"), (String::from("\u{a5}1,500"), true));
        assert_eq!(format_currency(-12.5, "USD", "en-US"), (String::from("-$12.50"), true));
    }

    #[test]
    fn unsupported_currency_falls_back_honestly() {
        let (formatted, supported) = format_currency(99.99, "XYZ", "en-US");
        assert!(!supported);
        assert_eq!(formatted, "XYZ 99.99");
    }

    #[test]
    fn unsupported_locale_falls_back_honestly() {
        let (formatted, supported) = format_currency(50.0, "USD", "xx-XX");
        assert!(!supported);
        assert_eq!(formatted, "USD 50.00");
    }

    #[test]
    fn zero_is_not_an_error_case() {
        assert_eq!(format_currency(0.0, "USD", "en-US"), (String::from("$0.00"), true));
    }

    #[test]
    fn locale_config_changes_formatting_for_same_amount() {
        let us = format_currency(1234.5, "EUR", "en-US");
        let de = format_currency(1234.5, "EUR", "de-DE");
        assert_ne!(us.0, de.0, "same amount, different locale must format differently");
    }

    #[test]
    fn output_genuinely_differs_across_distinct_inputs() {
        let out_a = handle(object(alloc::vec![
            ("amount", Value::Number(10.0)),
            ("currency_code", Value::String(String::from("USD"))),
        ]));
        let out_b = handle(object(alloc::vec![
            ("amount", Value::Number(20000.0)),
            ("currency_code", Value::String(String::from("USD"))),
        ]));
        assert_ne!(
            out_a.get("formatted").unwrap().as_str(),
            out_b.get("formatted").unwrap().as_str(),
            "output must depend on input, not be fixed"
        );
    }
}
