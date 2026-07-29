//! Real, deterministic implementation of the
//! `validation.score-password-strength` capability. Structural scoring only
//! -- no breach-database check (not possible from inside the WASM sandbox),
//! never echoes the password.
//!
//! The scoring model below was worked out during implementation: the
//! original SPEC.md draft's prose description and its own worked examples
//! didn't actually reconcile to one consistent algorithm (found by trying
//! to implement it, not assumed up front). This is the reconciled,
//! self-consistent version; SPEC.md was corrected to match real behavior
//! rather than the other way around.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use wasi_agent_runtime::{object, Value};

const DEFAULT_MIN_LENGTH: usize = 8;

struct ScoreResult {
    score: i32,
    strength: &'static str,
    issues: Vec<&'static str>,
}

fn is_symbol(c: char) -> bool {
    c.is_ascii_graphic() && !c.is_ascii_alphanumeric()
}

fn has_sequential_run(chars: &[char]) -> bool {
    if chars.len() < 4 {
        return false;
    }
    for window in chars.windows(4) {
        let codes: Vec<i32> = window.iter().map(|c| c.to_ascii_lowercase() as i32).collect();
        let ascending = codes[1] == codes[0] + 1 && codes[2] == codes[1] + 1 && codes[3] == codes[2] + 1;
        let descending = codes[1] == codes[0] - 1 && codes[2] == codes[1] - 1 && codes[3] == codes[2] - 1;
        if ascending || descending {
            return true;
        }
    }
    false
}

fn has_repeated_run(chars: &[char]) -> bool {
    if chars.len() < 4 {
        return false;
    }
    chars.windows(4).any(|w| w[0] == w[1] && w[1] == w[2] && w[2] == w[3])
}

fn score_password(password: &str, min_length: usize, require_symbol: bool) -> ScoreResult {
    let chars: Vec<char> = password.chars().collect();
    let char_count = chars.len();

    let has_length = char_count >= min_length;
    let has_lower = chars.iter().any(|c| c.is_ascii_lowercase());
    let has_upper = chars.iter().any(|c| c.is_ascii_uppercase());
    let has_digit = chars.iter().any(|c| c.is_ascii_digit());
    let has_symbol = chars.iter().any(|&c| is_symbol(c));

    let structural = i32::from(has_lower) + i32::from(has_upper) + i32::from(has_digit) + i32::from(has_symbol);
    let length_bonus = i32::from(char_count >= min_length.saturating_mul(2));
    let sequential = has_sequential_run(&chars);
    let repeated = has_repeated_run(&chars);
    let penalties = i32::from(sequential) + i32::from(repeated);

    let raw = structural + length_bonus - penalties;
    let mut score = if has_length { raw } else { raw.min(1) };
    if require_symbol && !has_symbol {
        score = score.min(3);
    }
    score = score.clamp(0, 4);

    let strength = match score {
        0 => "very_weak",
        1 => "weak",
        2 => "fair",
        3 => "strong",
        _ => "very_strong",
    };

    let mut issues = Vec::new();
    if !has_length {
        issues.push("too_short");
    }
    if !has_lower {
        issues.push("add_lowercase");
    }
    if !has_upper {
        issues.push("add_uppercase");
    }
    if !has_digit {
        issues.push("add_digit");
    }
    if !has_symbol {
        issues.push("add_symbol");
    }
    if sequential {
        issues.push("sequential_pattern");
    }
    if repeated {
        issues.push("repeated_characters");
    }

    ScoreResult { score, strength, issues }
}

fn handle(input: Value) -> Value {
    let password = input.get("password").and_then(Value::as_str).unwrap_or("");
    let min_length = input
        .get("min_length")
        .and_then(Value::as_f64)
        .map(|n| n as usize)
        .unwrap_or(DEFAULT_MIN_LENGTH);
    let require_symbol = input
        .get("require_symbol")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let result = score_password(password, min_length, require_symbol);
    let issues: Vec<String> = result.issues.iter().map(|s| String::from(*s)).collect();

    object(alloc::vec![
        ("score", Value::Number(result.score as f64)),
        ("strength", Value::String(String::from(result.strength))),
        ("issues", wasi_agent_runtime::array_of_strings(&issues)),
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

    fn score(password: &str) -> i32 {
        score_password(password, DEFAULT_MIN_LENGTH, false).score
    }

    #[test]
    fn very_strong_password() {
        let r = score_password("Tr@verse2026!", DEFAULT_MIN_LENGTH, false);
        assert_eq!(r.score, 4);
        assert_eq!(r.strength, "very_strong");
        assert!(r.issues.is_empty());
    }

    #[test]
    fn long_lowercase_only_gets_length_bonus() {
        let r = score_password("correcthorsebatterystaple", DEFAULT_MIN_LENGTH, false);
        assert_eq!(r.score, 2);
        assert_eq!(r.strength, "fair");
    }

    #[test]
    fn empty_password() {
        let r = score_password("", DEFAULT_MIN_LENGTH, false);
        assert_eq!(r.score, 0);
        assert!(r.issues.contains(&"too_short"));
    }

    #[test]
    fn common_weak_password() {
        assert_eq!(score("password"), 1);
    }

    #[test]
    fn sequential_pattern_penalized() {
        let r = score_password("12345678", DEFAULT_MIN_LENGTH, false);
        assert_eq!(r.score, 0);
        assert!(r.issues.contains(&"sequential_pattern"));
    }

    #[test]
    fn repeated_characters_penalized() {
        let r = score_password("aaaaaaaa", DEFAULT_MIN_LENGTH, false);
        assert_eq!(r.score, 0);
        assert!(r.issues.contains(&"repeated_characters"));
    }

    #[test]
    fn too_short_caps_score_even_with_diverse_characters() {
        let r = score_password("Short1!", 12, false);
        assert!(r.issues.contains(&"too_short"));
        assert!(r.score <= 1, "a too-short password should be capped low regardless of character diversity, got {}", r.score);
    }

    #[test]
    fn require_symbol_config_caps_score_when_missing() {
        // Lacking a symbol already caps structural score at 3 (only 3 of
        // the 4 character-class checks can pass) -- the require_symbol cap
        // only has an observable effect once the length bonus (>=2x
        // min_length) would otherwise push a symbol-less password to 4.
        let without_requirement = score_password("Tr4versetraverse", DEFAULT_MIN_LENGTH, false);
        let with_requirement = score_password("Tr4versetraverse", DEFAULT_MIN_LENGTH, true);
        assert_eq!(without_requirement.score, 4);
        assert_ne!(
            without_requirement.score, with_requirement.score,
            "same password, different require_symbol must differ"
        );
        assert_eq!(with_requirement.score, 3);
    }

    #[test]
    fn password_is_never_echoed_in_output() {
        let out = handle(object(alloc::vec![(
            "password",
            Value::String(String::from("SuperSecretValue123!")),
        )]));
        let written = wasi_agent_runtime::write_json(&out);
        assert!(!written.contains("SuperSecretValue123"));
    }

    #[test]
    fn output_genuinely_differs_across_distinct_inputs() {
        let out_a = handle(object(alloc::vec![(
            "password",
            Value::String(String::from("Tr@verse2026!")),
        )]));
        let out_b = handle(object(alloc::vec![(
            "password",
            Value::String(String::from("aaaa")),
        )]));
        assert_ne!(
            out_a.get("score").unwrap().as_f64(),
            out_b.get("score").unwrap().as_f64(),
            "output must depend on input, not be fixed"
        );
    }
}
