#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
extern crate alloc;
use alloc::string::String;
use wasi_capability_runtime::{object, Value};
fn text<'a>(v: &'a Value, n: &str) -> Option<&'a str> { v.get(n)?.as_str().filter(|x| !x.is_empty()) }
fn integer(v: &Value, n: &str) -> Option<i64> { let x=v.get(n)?.as_f64()?; let i=x as i64; (x.is_finite() && x >= 1.0 && x == i as f64).then_some(i) }
fn create(input: Value) -> Value {
    let artifact=input.get("artifact"); let policy=input.get("revision_policy");
    let id=artifact.and_then(|v| text(v,"id")); let reason=artifact.and_then(|v| text(v,"correction_reason")); let prior=artifact.and_then(|v| integer(v,"previous_revision")); let version=policy.and_then(|v| text(v,"version"));
    if id.is_none() || reason.is_none() || prior.is_none() || version.is_none() { return object(alloc::vec![("artifact_id",Value::String(String::new())),("revision_number",Value::Number(0.0)),("supersedes_revision",Value::Number(0.0)),("correction_reason",Value::String(String::new())),("policy_version",Value::String(String::new()))]); }
    let prior=prior.unwrap_or_default(); object(alloc::vec![("artifact_id",Value::String(String::from(id.unwrap_or_default()))),("revision_number",Value::Number((prior+1) as f64)),("supersedes_revision",Value::Number(prior as f64)),("correction_reason",Value::String(String::from(reason.unwrap_or_default()))),("policy_version",Value::String(String::from(version.unwrap_or_default())))])
}
#[cfg(not(test))]
#[unsafe(no_mangle)] pub extern "C" fn _start() { wasi_capability_runtime::run_capability(create); }
#[cfg(test)] mod tests { use super::*; fn req() -> Value { object(alloc::vec![("artifact",object(alloc::vec![("id",Value::String(String::from("canvas"))), ("previous_revision",Value::Number(1.0)), ("correction_reason",Value::String(String::from("verified")))])), ("revision_policy",object(alloc::vec![("version",Value::String(String::from("1.0.0")))]))]) } #[test] fn creates_successor(){assert_eq!(create(req()).get("revision_number").and_then(Value::as_f64),Some(2.0));} #[test] fn preserves_superseded_revision(){assert_eq!(create(req()).get("supersedes_revision").and_then(Value::as_f64),Some(1.0));} #[test] fn rejects_incomplete(){let x=object(alloc::vec![]);assert_eq!(create(x).get("revision_number").and_then(Value::as_f64),Some(0.0));} }
