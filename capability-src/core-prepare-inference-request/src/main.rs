#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;
use alloc::string::String;
use wasi_capability_runtime::{object, Value};

fn field<'a>(input: &'a Value, name: &str) -> Option<&'a str> {
    input.get(name)?.as_str().filter(|v| !v.is_empty())
}

fn response(input: &Value, class: &str) -> Value {
    let valid = class == "planned";
    object(alloc::vec![
        ("connector_id", Value::String(String::from(if valid { "traverse.local-model-runtime" } else { "" }))),
        ("operation", Value::String(String::from(if valid { "infer" } else { "" }))),
        ("input_artifact_ref", Value::String(String::from(if valid { field(input, "input_artifact_ref").unwrap_or("") } else { "" }))),
        ("model_artifact_ref", Value::String(String::from(if valid { field(input, "model_artifact_ref").unwrap_or("") } else { "" }))),
        ("inference_profile_ref", Value::String(String::from(if valid { field(input, "inference_profile_ref").unwrap_or("") } else { "" }))),
        ("idempotency_key", Value::String(String::from(if valid { field(input, "idempotency_key").unwrap_or("") } else { "" }))),
        ("result_class", Value::String(String::from(class))),
    ])
}

fn prepare(input: Value) -> Value {
    let valid = ["input_artifact_ref", "model_artifact_ref", "inference_profile_ref", "idempotency_key"]
        .iter().all(|name| field(&input, name).is_some());
    response(&input, if valid { "planned" } else { "invalid_request" })
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() { wasi_capability_runtime::run_capability(prepare); }

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> Value { object(alloc::vec![
        ("input_artifact_ref", Value::String(String::from("asset:input"))),
        ("model_artifact_ref", Value::String(String::from("model:release"))),
        ("inference_profile_ref", Value::String(String::from("profile:default"))),
        ("idempotency_key", Value::String(String::from("run-1"))),
    ]) }
    #[test] fn plans_complete_request() { assert_eq!(prepare(request()).get("result_class").and_then(Value::as_str), Some("planned")); }
    #[test] fn rejects_missing_reference() { let input = object(alloc::vec![("input_artifact_ref", Value::String(String::from("asset:x")))]); assert_eq!(prepare(input).get("result_class").and_then(Value::as_str), Some("invalid_request")); }
    #[test] fn planned_output_is_connector_neutral() { let out = prepare(request()); assert_eq!(out.get("connector_id").and_then(Value::as_str), Some("traverse.local-model-runtime")); assert_eq!(out.get("operation").and_then(Value::as_str), Some("infer")); }
}
