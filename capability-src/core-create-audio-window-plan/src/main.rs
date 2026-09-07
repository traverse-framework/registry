//! Portable planning of overlapping analysis windows for a declared-duration
//! artifact. This is calculation only: it never reads bytes, selects a codec
//! or model, invokes a connector, or knows an application workflow.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::string::String;
use wasi_capability_runtime::{object, Value};

fn positive_integer(value: Option<&Value>) -> Option<i64> {
    let number = value?.as_f64()?;
    if !number.is_finite() || number < 1.0 || number > i64::MAX as f64 {
        return None;
    }
    let integer = number as i64;
    (number == integer as f64).then_some(integer)
}

fn string_field<'a>(object: Option<&'a Value>, name: &str) -> Option<&'a str> {
    object?.get(name)?.as_str().filter(|value| !value.is_empty())
}

fn response(
    artifact_ref: &str,
    window_millis: i64,
    hop_millis: i64,
    window_count: i64,
    policy_version: &str,
    result_class: &str,
) -> Value {
    object(alloc::vec![
        ("artifact_ref", Value::String(String::from(artifact_ref))),
        ("window_millis", Value::Number(window_millis as f64)),
        ("hop_millis", Value::Number(hop_millis as f64)),
        ("window_count", Value::Number(window_count as f64)),
        ("policy_version", Value::String(String::from(policy_version))),
        ("result_class", Value::String(String::from(result_class))),
    ])
}

fn create_window_plan(input: Value) -> Value {
    let artifact_ref = string_field(Some(&input), "artifact_ref").unwrap_or("");
    let duration = positive_integer(input.get("duration_millis"));
    let policy = input.get("window_policy");
    let policy_version = string_field(policy, "policy_version").unwrap_or("");
    let window = positive_integer(policy.and_then(|value| value.get("window_millis")));
    let hop = positive_integer(policy.and_then(|value| value.get("hop_millis")));

    if artifact_ref.is_empty() || duration.is_none() || policy_version.is_empty() {
        return response(artifact_ref, 0, 0, 0, policy_version, "invalid_request");
    }
    if window.is_none() || hop.is_none() || hop > window {
        return response(artifact_ref, 0, 0, 0, policy_version, "invalid_window_policy");
    }

    let duration = duration.unwrap_or_default();
    let window = window.unwrap_or_default();
    let hop = hop.unwrap_or_default();
    let count = if duration <= window { 1 } else { 1 + (duration - window) / hop };
    response(artifact_ref, window, hop, count, policy_version, "planned")
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    wasi_capability_runtime::run_capability(create_window_plan);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(artifact_ref: &str, duration: Value, version: &str, window: Value, hop: Value) -> Value {
        object(alloc::vec![
            ("artifact_ref", Value::String(String::from(artifact_ref))),
            ("duration_millis", duration),
            ("window_policy", object(alloc::vec![
                ("policy_version", Value::String(String::from(version))),
                ("window_millis", window),
                ("hop_millis", hop),
            ])),
        ])
    }

    fn result(output: &Value) -> Option<&str> {
        output.get("result_class").and_then(Value::as_str)
    }

    #[test]
    fn creates_overlapping_plan() {
        let output = create_window_plan(request("asset:pcm-17", Value::Number(60_000.0), "v1", Value::Number(5_000.0), Value::Number(2_500.0)));
        assert_eq!(result(&output), Some("planned"));
        assert_eq!(output.get("window_count").and_then(Value::as_f64), Some(23.0));
    }

    #[test]
    fn creates_one_window_when_duration_is_not_longer_than_window() {
        let output = create_window_plan(request("asset:x", Value::Number(5_000.0), "v1", Value::Number(5_000.0), Value::Number(2_500.0)));
        assert_eq!(output.get("window_count").and_then(Value::as_f64), Some(1.0));
    }

    #[test]
    fn rejects_invalid_window_geometry() {
        let output = create_window_plan(request("asset:x", Value::Number(6_000.0), "v1", Value::Number(5_000.0), Value::Number(6_000.0)));
        assert_eq!(result(&output), Some("invalid_window_policy"));

        let zero_hop = create_window_plan(request("asset:x", Value::Number(6_000.0), "v1", Value::Number(5_000.0), Value::Number(0.0)));
        assert_eq!(result(&zero_hop), Some("invalid_window_policy"));
    }

    #[test]
    fn rejects_invalid_request_fields() {
        let missing_duration = create_window_plan(object(alloc::vec![("artifact_ref", Value::String(String::from("asset:x")))]));
        assert_eq!(result(&missing_duration), Some("invalid_request"));

        let fractional_duration = create_window_plan(request("asset:x", Value::Number(10.5), "v1", Value::Number(5_000.0), Value::Number(2_500.0)));
        assert_eq!(result(&fractional_duration), Some("invalid_request"));
    }

    #[test]
    fn helpers_reject_non_positive_or_empty_values() {
        assert_eq!(positive_integer(Some(&Value::Number(1.0))), Some(1));
        assert_eq!(positive_integer(Some(&Value::Number(0.0))), None);
        assert_eq!(positive_integer(Some(&Value::Number(1.5))), None);
        assert_eq!(string_field(None, "missing"), None);
    }
}
