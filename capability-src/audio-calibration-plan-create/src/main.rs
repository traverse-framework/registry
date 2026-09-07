//! Generic bounded audio calibration planning.
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
extern crate alloc;
use alloc::string::String;
use wasi_capability_runtime::{object, Value};

fn integer(v: Option<&Value>) -> Option<i64> { let n=v?.as_f64()?; (n.is_finite()&&n==n as i64 as f64).then_some(n as i64) }
fn text<'a>(v: Option<&'a Value>, key:&str)->Option<&'a str>{v?.get(key)?.as_str().filter(|s|!s.is_empty())}
fn out(id:&str,target:i64,tol:i64,dur:i64,key:&str,result:&str)->Value{object(alloc::vec![
 ("connector_id",Value::String(String::from(if result=="planned"{"traverse.audio-input"}else{""}))),
 ("operation",Value::String(String::from(if result=="planned"{"calibrate"}else{""}))),
 ("plan_id",Value::String(String::from(id))), ("target_level_dbfs",Value::Number(target as f64)),
 ("tolerance_db",Value::Number(tol as f64)), ("duration_seconds",Value::Number(dur as f64)),
 ("idempotency_key",Value::String(String::from(if result=="planned"{key}else{""}))),
 ("result_class",Value::String(String::from(result)))])}
fn plan(input:Value)->Value{let id=text(Some(&input),"plan_id").unwrap_or("");let key=text(Some(&input),"idempotency_key").unwrap_or("");let target=integer(input.get("target_level_dbfs")).unwrap_or(999);let tol=integer(input.get("tolerance_db")).unwrap_or(0);let dur=integer(input.get("duration_seconds")).unwrap_or(0);if id.is_empty()||key.is_empty(){return out(id,0,0,0,"","invalid_request")}if !(-60..=0).contains(&target){return out(id,0,0,0,"","target_out_of_bounds")}if !(1..=20).contains(&tol){return out(id,0,0,0,"","tolerance_out_of_bounds")}if !(1..=300).contains(&dur){return out(id,0,0,0,"","duration_out_of_bounds")}out(id,target,tol,dur,key,"planned")}
#[cfg(not(test))]#[unsafe(no_mangle)]pub extern "C" fn _start(){wasi_capability_runtime::run_capability(plan)}
#[cfg(test)]mod tests{use super::*;fn req(t:f64,tol:f64,d:f64)->Value{object(alloc::vec![("plan_id",Value::String(String::from("x"))), ("target_level_dbfs",Value::Number(t)), ("tolerance_db",Value::Number(tol)), ("duration_seconds",Value::Number(d)), ("idempotency_key",Value::String(String::from("x")))])}fn class(v:&Value)->Option<&str>{v.get("result_class").and_then(Value::as_str)}#[test]fn plans_valid(){assert_eq!(class(&plan(req(-18.0,3.0,10.0))),Some("planned"))}#[test]fn rejects_target(){assert_eq!(class(&plan(req(2.0,3.0,10.0))),Some("target_out_of_bounds"))}#[test]fn rejects_tolerance(){assert_eq!(class(&plan(req(-18.0,0.0,10.0))),Some("tolerance_out_of_bounds"))}#[test]fn rejects_duration(){assert_eq!(class(&plan(req(-18.0,3.0,301.0))),Some("duration_out_of_bounds"))}#[test]fn rejects_missing_ids(){let v=object(alloc::vec![("plan_id",Value::String(String::new())),("idempotency_key",Value::String(String::new()))]);assert_eq!(class(&plan(v)),Some("invalid_request"))}}
