#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
extern crate alloc;
use alloc::string::String;
use wasi_capability_runtime::{object, Value};

fn text<'a>(input: &'a Value, name: &str) -> Option<&'a str> { input.get(name)?.as_str().filter(|v| !v.is_empty()) }
fn output(input: &Value, class: &str) -> Value {
    let planned = class == "planned";
    object(alloc::vec![
        ("connector_id", Value::String(String::from(if planned { "traverse.scheduler" } else { "" }))),
        ("operation", Value::String(String::from(if planned { "schedule_invocation" } else { "" }))),
        ("job_kind", Value::String(String::from(if planned { text(input, "job_kind").unwrap_or("") } else { "" }))),
        ("calendar_policy_ref", Value::String(String::from(if planned { text(input, "calendar_policy_ref").unwrap_or("") } else { "" }))),
        ("logical_deadline", Value::String(String::from(if planned { text(input, "logical_deadline").unwrap_or("") } else { "" }))),
        ("idempotency_key", Value::String(String::from(if planned { text(input, "idempotency_key").unwrap_or("") } else { "" }))),
        ("result_class", Value::String(String::from(class))),
    ])
}
fn evaluate(input: Value) -> Value {
    let valid = ["trigger_id", "job_kind", "calendar_policy_ref", "logical_deadline", "idempotency_key"].iter().all(|n| text(&input, n).is_some());
    if !valid || input.get("is_due").and_then(Value::as_bool).is_none() { return output(&input, "invalid_request"); }
    if input.get("is_due").and_then(Value::as_bool) == Some(false) { output(&input, "not_due") } else { output(&input, "planned") }
}
#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() { wasi_capability_runtime::run_capability(evaluate); }
#[cfg(test)]
mod tests {
    use super::*;
    fn request(due: bool) -> Value { object(alloc::vec![
        ("trigger_id", Value::String(String::from("trigger:daily"))), ("job_kind", Value::String(String::from("close-period"))),
        ("calendar_policy_ref", Value::String(String::from("policy:local-day"))), ("logical_deadline", Value::String(String::from("2026-09-06T06:00:00Z"))),
        ("is_due", Value::Bool(due)), ("idempotency_key", Value::String(String::from("close-1"))),
    ]) }
    #[test] fn plans_due_trigger() { assert_eq!(evaluate(request(true)).get("result_class").and_then(Value::as_str), Some("planned")); }
    #[test] fn leaves_not_due_unscheduled() { assert_eq!(evaluate(request(false)).get("result_class").and_then(Value::as_str), Some("not_due")); }
    #[test] fn rejects_incomplete_context() { let input = object(alloc::vec![("is_due", Value::Bool(true))]); assert_eq!(evaluate(input).get("result_class").and_then(Value::as_str), Some("invalid_request")); }
}
