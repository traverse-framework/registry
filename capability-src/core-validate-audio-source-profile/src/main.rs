//! Portable validation for an audio-source profile and its requested bounded
//! capture duration. This is policy evaluation only: it never discovers a
//! device, requests permission, captures audio, or invokes a connector.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::string::String;
use wasi_capability_runtime::{object, Value};

const MIN_SAMPLE_RATE_HZ: i64 = 8_000;
const MAX_SAMPLE_RATE_HZ: i64 = 192_000;
const MAX_CHANNEL_COUNT: i64 = 2;

fn positive_integer(value: Option<&Value>) -> Option<i64> {
    let number = value?.as_f64()?;
    if !number.is_finite() || number < 1.0 || number > i64::MAX as f64 {
        return None;
    }
    let integer = number as i64;
    if number != integer as f64 {
        return None;
    }
    Some(integer)
}

fn string_field<'a>(object: Option<&'a Value>, name: &str) -> Option<&'a str> {
    object?.get(name)?.as_str().filter(|value| !value.is_empty())
}

fn compatible_encoding(encoding: &str, bit_depth: i64) -> bool {
    matches!(
        (encoding, bit_depth),
        ("pcm_s16le", 16) | ("pcm_s24le", 24) | ("pcm_f32le", 32)
    )
}

fn response(valid: bool, reason_code: &str, policy_version: &str) -> Value {
    object(alloc::vec![
        ("valid", Value::Bool(valid)),
        ("reason_code", Value::String(String::from(reason_code))),
        ("policy_version", Value::String(String::from(policy_version))),
    ])
}

fn validate_profile(input: Value) -> Value {
    let profile = input.get("source_profile");
    let policy = input.get("capture_policy");
    let policy_version = string_field(policy, "policy_version").unwrap_or("");
    let maximum = positive_integer(policy.and_then(|value| value.get("max_segment_seconds")));
    let requested = positive_integer(policy.and_then(|value| value.get("requested_segment_seconds")));
    let channels = positive_integer(profile.and_then(|value| value.get("channel_count")));
    let rate = positive_integer(profile.and_then(|value| value.get("sample_rate_hz")));
    let depth = positive_integer(profile.and_then(|value| value.get("bit_depth")));
    let encoding = string_field(profile, "encoding");

    if policy_version.is_empty() || maximum.is_none() || requested.is_none() {
        return response(false, "invalid_request", policy_version);
    }
    if channels.is_none_or(|value| value > MAX_CHANNEL_COUNT) {
        return response(false, "unsupported_channel_count", policy_version);
    }
    if rate.is_none_or(|value| !(MIN_SAMPLE_RATE_HZ..=MAX_SAMPLE_RATE_HZ).contains(&value)) {
        return response(false, "unsupported_sample_rate", policy_version);
    }
    if !matches!(depth, Some(16 | 24 | 32)) {
        return response(false, "unsupported_bit_depth", policy_version);
    }
    if encoding.is_none_or(|value| !compatible_encoding(value, depth.unwrap_or_default())) {
        return response(false, "unsupported_encoding", policy_version);
    }
    if requested.unwrap_or_default() > maximum.unwrap_or_default() {
        return response(false, "segment_duration_exceeds_policy", policy_version);
    }

    response(true, "source_profile_compatible", policy_version)
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    wasi_capability_runtime::run_capability(validate_profile);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(rate: i64, channels: i64, depth: i64, encoding: &str) -> Value {
        object(alloc::vec![
            ("sample_rate_hz", Value::Number(rate as f64)),
            ("channel_count", Value::Number(channels as f64)),
            ("bit_depth", Value::Number(depth as f64)),
            ("encoding", Value::String(String::from(encoding))),
        ])
    }

    fn policy(version: &str, maximum: i64, requested: i64) -> Value {
        object(alloc::vec![
            ("policy_version", Value::String(String::from(version))),
            ("max_segment_seconds", Value::Number(maximum as f64)),
            ("requested_segment_seconds", Value::Number(requested as f64)),
        ])
    }

    fn request(profile_value: Value, policy_value: Value) -> Value {
        object(alloc::vec![
            ("source_profile", profile_value),
            ("capture_policy", policy_value),
        ])
    }

    fn reason(output: &Value) -> Option<&str> {
        output.get("reason_code").and_then(Value::as_str)
    }

    #[test]
    fn accepts_each_supported_pcm_profile() {
        for (depth, encoding) in [(16, "pcm_s16le"), (24, "pcm_s24le"), (32, "pcm_f32le")] {
            let output = validate_profile(request(profile(48_000, 1, depth, encoding), policy("v1", 900, 900)));
            assert_eq!(output.get("valid").and_then(Value::as_bool), Some(true));
            assert_eq!(reason(&output), Some("source_profile_compatible"));
        }
    }

    #[test]
    fn rejects_missing_or_invalid_policy_fields() {
        let missing_policy = validate_profile(object(alloc::vec![("source_profile", profile(48_000, 1, 16, "pcm_s16le"))]));
        assert_eq!(reason(&missing_policy), Some("invalid_request"));

        let empty_version = validate_profile(request(profile(48_000, 1, 16, "pcm_s16le"), policy("", 900, 900)));
        assert_eq!(reason(&empty_version), Some("invalid_request"));

        let fractional_duration = validate_profile(request(profile(48_000, 1, 16, "pcm_s16le"), object(alloc::vec![
            ("policy_version", Value::String(String::from("v1"))),
            ("max_segment_seconds", Value::Number(900.5)),
            ("requested_segment_seconds", Value::Number(900.0)),
        ])));
        assert_eq!(reason(&fractional_duration), Some("invalid_request"));
    }

    #[test]
    fn rejects_unsupported_channel_count_and_rate() {
        let too_many_channels = validate_profile(request(profile(48_000, 3, 16, "pcm_s16le"), policy("v1", 900, 900)));
        assert_eq!(reason(&too_many_channels), Some("unsupported_channel_count"));

        let too_low_rate = validate_profile(request(profile(7_999, 1, 16, "pcm_s16le"), policy("v1", 900, 900)));
        assert_eq!(reason(&too_low_rate), Some("unsupported_sample_rate"));

        let too_high_rate = validate_profile(request(profile(192_001, 1, 16, "pcm_s16le"), policy("v1", 900, 900)));
        assert_eq!(reason(&too_high_rate), Some("unsupported_sample_rate"));
    }

    #[test]
    fn rejects_unsupported_bit_depth_and_encoding_pair() {
        let unsupported_depth = validate_profile(request(profile(48_000, 1, 12, "pcm_s16le"), policy("v1", 900, 900)));
        assert_eq!(reason(&unsupported_depth), Some("unsupported_bit_depth"));

        let incompatible_pair = validate_profile(request(profile(48_000, 1, 16, "pcm_f32le"), policy("v1", 900, 900)));
        assert_eq!(reason(&incompatible_pair), Some("unsupported_encoding"));
    }

    #[test]
    fn rejects_a_duration_over_the_declared_maximum() {
        let output = validate_profile(request(profile(48_000, 1, 16, "pcm_s16le"), policy("v1", 900, 901)));
        assert_eq!(reason(&output), Some("segment_duration_exceeds_policy"));
    }

    #[test]
    fn helpers_have_explicit_boundary_behavior() {
        assert_eq!(positive_integer(Some(&Value::Number(1.0))), Some(1));
        assert_eq!(positive_integer(Some(&Value::Number(0.0))), None);
        assert_eq!(positive_integer(Some(&Value::Number(-1.0))), None);
        assert_eq!(positive_integer(Some(&Value::Number(1.5))), None);
        assert_eq!(positive_integer(Some(&Value::String(String::from("1")))), None);
        assert_eq!(string_field(None, "anything"), None);
        assert_eq!(string_field(Some(&object(alloc::vec![("x", Value::String(String::new()))])), "x"), None);
        assert!(compatible_encoding("pcm_s16le", 16));
        assert!(!compatible_encoding("pcm_s16le", 24));
    }
}
