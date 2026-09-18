#![cfg_attr(all(target_arch = "wasm32", not(test)), no_std)]
#![cfg_attr(all(target_arch = "wasm32", not(test)), no_main)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use wasi_capability_runtime::{object, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Timestamp {
    epoch_seconds: i64,
    nanos: u32,
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn days_in_month(year: i64, month: u32) -> Option<u32> {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => Some(31),
        4 | 6 | 9 | 11 => Some(30),
        2 => {
            if is_leap_year(year) {
                Some(29)
            } else {
                Some(28)
            }
        }
        _ => None,
    }
}

fn days_from_civil(mut y: i64, m: u32, d: u32) -> i64 {
    y -= if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + (doe as i64) - 719468
}

fn parse_u32(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() {
        return None;
    }
    let mut val: u32 = 0;
    for &b in bytes {
        if !b.is_ascii_digit() {
            return None;
        }
        val = val.checked_mul(10).and_then(|v| v.checked_add((b - b'0') as u32))?;
    }
    Some(val)
}

fn parse_timestamp(s: &str) -> Option<Timestamp> {
    let bytes = s.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    if bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    if bytes[10] != b'T' && bytes[10] != b't' {
        return None;
    }
    if bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }

    let year = parse_u32(&bytes[0..4])? as i64;
    let month = parse_u32(&bytes[5..7])?;
    let day = parse_u32(&bytes[8..10])?;
    let hour = parse_u32(&bytes[11..13])?;
    let minute = parse_u32(&bytes[14..16])?;
    let second = parse_u32(&bytes[17..19])?;

    if !(1..=12).contains(&month) {
        return None;
    }
    let max_days = days_in_month(year, month)?;
    if day < 1 || day > max_days {
        return None;
    }
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }

    let mut idx = 19;
    let mut nanos: u32 = 0;
    if idx < bytes.len() && (bytes[idx] == b'.' || bytes[idx] == b',') {
        idx += 1;
        let frac_start = idx;
        while idx < bytes.len() && bytes[idx].is_ascii_digit() {
            idx += 1;
        }
        let frac_digits = &bytes[frac_start..idx];
        if frac_digits.is_empty() {
            return None;
        }
        let mut n: u32 = 0;
        let mut scale: u32 = 100_000_000;
        for &d in frac_digits.iter().take(9) {
            n = n.checked_add(((d - b'0') as u32).saturating_mul(scale))?;
            scale /= 10;
        }
        nanos = n;
    }

    if idx >= bytes.len() {
        return None;
    }

    let offset_seconds: i64;
    if bytes[idx] == b'Z' || bytes[idx] == b'z' {
        idx += 1;
        offset_seconds = 0;
    } else if bytes[idx] == b'+' || bytes[idx] == b'-' {
        let sign: i64 = if bytes[idx] == b'+' { 1 } else { -1 };
        idx += 1;
        let tz_bytes = &bytes[idx..];
        if tz_bytes.len() == 5 && tz_bytes[2] == b':' {
            let tz_hour = parse_u32(&tz_bytes[0..2])? as i64;
            let tz_min = parse_u32(&tz_bytes[3..5])? as i64;
            if tz_hour > 23 || tz_min > 59 {
                return None;
            }
            offset_seconds = sign * (tz_hour * 3600 + tz_min * 60);
            idx += 5;
        } else if tz_bytes.len() == 4 {
            let tz_hour = parse_u32(&tz_bytes[0..2])? as i64;
            let tz_min = parse_u32(&tz_bytes[2..4])? as i64;
            if tz_hour > 23 || tz_min > 59 {
                return None;
            }
            offset_seconds = sign * (tz_hour * 3600 + tz_min * 60);
            idx += 4;
        } else if tz_bytes.len() == 2 {
            let tz_hour = parse_u32(&tz_bytes[0..2])? as i64;
            if tz_hour > 23 {
                return None;
            }
            offset_seconds = sign * (tz_hour * 3600);
            idx += 2;
        } else {
            return None;
        }
    } else {
        return None;
    }

    if idx != bytes.len() {
        return None;
    }

    let days = days_from_civil(year, month, day);
    let total_seconds = days
        .checked_mul(86400)
        .and_then(|s| s.checked_add((hour as i64) * 3600))
        .and_then(|s| s.checked_add((minute as i64) * 60))
        .and_then(|s| s.checked_add(second as i64))
        .and_then(|s| s.checked_sub(offset_seconds))?;

    Some(Timestamp {
        epoch_seconds: total_seconds,
        nanos,
    })
}

fn get_str<'a>(val: &'a Value, key: &str) -> Option<&'a str> {
    val.get(key)?.as_str()
}

fn build_response(
    decision: &str,
    reason_code: &str,
    matched_consent_index: Option<usize>,
    policy_version: &str,
) -> Value {
    let index_val = match matched_consent_index {
        Some(idx) => Value::Number(idx as f64),
        None => Value::Null,
    };
    object(alloc::vec![
        ("decision", Value::String(String::from(decision))),
        ("matched_consent_index", index_val),
        ("policy_version", Value::String(String::from(policy_version))),
        ("reason_code", Value::String(String::from(reason_code))),
    ])
}

pub fn evaluate(input: Value) -> Value {
    let policy_version = get_str(&input, "policy_version").unwrap_or("");
    let subject_id = match get_str(&input, "subject_id") {
        Some(s) if !s.is_empty() => s,
        _ => return build_response("deny", "invalid_input", None, policy_version),
    };
    let _ = subject_id;

    let requested_purpose = match get_str(&input, "requested_purpose") {
        Some(p) if !p.is_empty() => p,
        _ => return build_response("deny", "invalid_input", None, policy_version),
    };

    if policy_version.is_empty() {
        return build_response("deny", "invalid_input", None, policy_version);
    }

    let now_str = match get_str(&input, "now") {
        Some(n) if !n.is_empty() => n,
        _ => return build_response("deny", "invalid_input", None, policy_version),
    };

    let now_ts = match parse_timestamp(now_str) {
        Some(ts) => ts,
        None => return build_response("deny", "invalid_input", None, policy_version),
    };

    let consents_arr = match input.get("consents").and_then(Value::as_array) {
        Some(arr) => arr,
        None => return build_response("deny", "invalid_input", None, policy_version),
    };

    struct ValidatedConsent<'a> {
        index: usize,
        purpose: &'a str,
        status: &'a str,
        granted_at: Option<Timestamp>,
        expires_at: Option<Timestamp>,
    }

    let mut validated_consents = Vec::with_capacity(consents_arr.len());

    for (idx, item) in consents_arr.iter().enumerate() {
        let purpose = match get_str(item, "purpose") {
            Some(p) if !p.is_empty() => p,
            _ => return build_response("deny", "invalid_input", None, policy_version),
        };
        let status = match get_str(item, "status") {
            Some(s) if s == "granted" || s == "denied" || s == "withdrawn" => s,
            _ => return build_response("deny", "invalid_input", None, policy_version),
        };

        let granted_at = match item.get("granted_at") {
            None | Some(Value::Null) => None,
            Some(Value::String(ref s)) => match parse_timestamp(s) {
                Some(ts) => Some(ts),
                None => return build_response("deny", "invalid_input", None, policy_version),
            },
            _ => return build_response("deny", "invalid_input", None, policy_version),
        };

        let expires_at = match item.get("expires_at") {
            None | Some(Value::Null) => None,
            Some(Value::String(ref s)) => match parse_timestamp(s) {
                Some(ts) => Some(ts),
                None => return build_response("deny", "invalid_input", None, policy_version),
            },
            _ => return build_response("deny", "invalid_input", None, policy_version),
        };

        if let Some(scope_val) = item.get("scope") {
            match scope_val {
                Value::Null => {}
                Value::Array(ref arr) => {
                    for v in arr {
                        if v.as_str().is_none() {
                            return build_response("deny", "invalid_input", None, policy_version);
                        }
                    }
                }
                _ => return build_response("deny", "invalid_input", None, policy_version),
            }
        }

        validated_consents.push(ValidatedConsent {
            index: idx,
            purpose,
            status,
            granted_at,
            expires_at,
        });
    }

    for consent in validated_consents {
        if consent.purpose == requested_purpose {
            match consent.status {
                "withdrawn" => {
                    return build_response(
                        "deny",
                        "consent_withdrawn",
                        Some(consent.index),
                        policy_version,
                    );
                }
                "denied" => {
                    return build_response(
                        "deny",
                        "consent_denied",
                        Some(consent.index),
                        policy_version,
                    );
                }
                "granted" => {
                    if let Some(granted) = consent.granted_at {
                        if now_ts < granted {
                            return build_response("deny", "invalid_input", None, policy_version);
                        }
                    }
                    if let Some(expires) = consent.expires_at {
                        if now_ts >= expires {
                            return build_response(
                                "deny",
                                "consent_expired",
                                Some(consent.index),
                                policy_version,
                            );
                        }
                    }
                    return build_response(
                        "allow",
                        "consent_granted",
                        Some(consent.index),
                        policy_version,
                    );
                }
                _ => {
                    return build_response("deny", "invalid_input", None, policy_version);
                }
            }
        }
    }

    build_response("deny", "purpose_not_granted", None, policy_version)
}

#[cfg(all(target_arch = "wasm32", not(test)))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    wasi_capability_runtime::run_capability(evaluate);
}

#[cfg(all(not(target_arch = "wasm32"), not(test)))]
fn main() {}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request(
        subject_id: &str,
        requested_purpose: &str,
        now: &str,
        policy_version: &str,
        consents: Vec<Value>,
    ) -> Value {
        object(alloc::vec![
            ("subject_id", Value::String(String::from(subject_id))),
            ("requested_purpose", Value::String(String::from(requested_purpose))),
            ("now", Value::String(String::from(now))),
            ("policy_version", Value::String(String::from(policy_version))),
            ("consents", Value::Array(consents)),
        ])
    }

    fn make_consent(
        purpose: &str,
        status: &str,
        granted_at: Option<&str>,
        expires_at: Option<&str>,
        scope: Option<Vec<&str>>,
    ) -> Value {
        let mut fields = alloc::vec![
            ("purpose", Value::String(String::from(purpose))),
            ("status", Value::String(String::from(status))),
        ];
        if let Some(g) = granted_at {
            fields.push(("granted_at", Value::String(String::from(g))));
        }
        if let Some(e) = expires_at {
            fields.push(("expires_at", Value::String(String::from(e))));
        }
        if let Some(s) = scope {
            let s_vals = s.into_iter().map(|item| Value::String(String::from(item))).collect();
            fields.push(("scope", Value::Array(s_vals)));
        }
        object(fields)
    }

    #[test]
    fn allows_valid_granted_consent() {
        let req = make_request(
            "sub-1",
            "marketing",
            "2026-08-15T12:00:00Z",
            "1.0.0",
            alloc::vec![make_consent(
                "marketing",
                "granted",
                Some("2026-08-01T00:00:00Z"),
                Some("2026-09-01T00:00:00Z"),
                Some(alloc::vec!["email", "sms"]),
            )],
        );
        let resp = evaluate(req);
        assert_eq!(resp.get("decision").and_then(Value::as_str), Some("allow"));
        assert_eq!(resp.get("reason_code").and_then(Value::as_str), Some("consent_granted"));
        assert_eq!(resp.get("matched_consent_index").and_then(Value::as_f64), Some(0.0));
        assert_eq!(resp.get("policy_version").and_then(Value::as_str), Some("1.0.0"));
    }

    #[test]
    fn allows_granted_consent_without_expiry() {
        let req = make_request(
            "sub-1",
            "essential",
            "2026-08-15T12:00:00Z",
            "1.0.0",
            alloc::vec![make_consent(
                "essential",
                "granted",
                None,
                None,
                None,
            )],
        );
        let resp = evaluate(req);
        assert_eq!(resp.get("decision").and_then(Value::as_str), Some("allow"));
        assert_eq!(resp.get("reason_code").and_then(Value::as_str), Some("consent_granted"));
        assert_eq!(resp.get("matched_consent_index").and_then(Value::as_f64), Some(0.0));
    }

    #[test]
    fn denies_withdrawn_consent() {
        let req = make_request(
            "sub-2",
            "analytics",
            "2026-08-15T12:00:00Z",
            "1.0.0",
            alloc::vec![make_consent(
                "analytics",
                "withdrawn",
                Some("2026-08-01T00:00:00Z"),
                None,
                None,
            )],
        );
        let resp = evaluate(req);
        assert_eq!(resp.get("decision").and_then(Value::as_str), Some("deny"));
        assert_eq!(resp.get("reason_code").and_then(Value::as_str), Some("consent_withdrawn"));
        assert_eq!(resp.get("matched_consent_index").and_then(Value::as_f64), Some(0.0));
    }

    #[test]
    fn denies_explicitly_denied_consent() {
        let req = make_request(
            "sub-3",
            "cross-border-sharing",
            "2026-08-15T12:00:00Z",
            "1.0.0",
            alloc::vec![make_consent(
                "cross-border-sharing",
                "denied",
                Some("2026-08-01T00:00:00Z"),
                None,
                None,
            )],
        );
        let resp = evaluate(req);
        assert_eq!(resp.get("decision").and_then(Value::as_str), Some("deny"));
        assert_eq!(resp.get("reason_code").and_then(Value::as_str), Some("consent_denied"));
        assert_eq!(resp.get("matched_consent_index").and_then(Value::as_f64), Some(0.0));
    }

    #[test]
    fn denies_expired_consent() {
        let req = make_request(
            "sub-4",
            "promotions",
            "2026-09-02T00:00:00Z",
            "1.0.0",
            alloc::vec![make_consent(
                "promotions",
                "granted",
                Some("2026-08-01T00:00:00Z"),
                Some("2026-09-01T00:00:00Z"),
                None,
            )],
        );
        let resp = evaluate(req);
        assert_eq!(resp.get("decision").and_then(Value::as_str), Some("deny"));
        assert_eq!(resp.get("reason_code").and_then(Value::as_str), Some("consent_expired"));
        assert_eq!(resp.get("matched_consent_index").and_then(Value::as_f64), Some(0.0));
    }

    #[test]
    fn denies_when_purpose_not_found() {
        let req = make_request(
            "sub-5",
            "telemetry",
            "2026-08-15T12:00:00Z",
            "1.0.0",
            alloc::vec![make_consent(
                "billing",
                "granted",
                None,
                None,
                None,
            )],
        );
        let resp = evaluate(req);
        assert_eq!(resp.get("decision").and_then(Value::as_str), Some("deny"));
        assert_eq!(resp.get("reason_code").and_then(Value::as_str), Some("purpose_not_granted"));
        assert_eq!(resp.get("matched_consent_index"), Some(&Value::Null));
    }

    #[test]
    fn denies_on_future_granted_at() {
        let req = make_request(
            "sub-6",
            "marketing",
            "2026-08-01T00:00:00Z",
            "1.0.0",
            alloc::vec![make_consent(
                "marketing",
                "granted",
                Some("2026-08-10T00:00:00Z"),
                None,
                None,
            )],
        );
        let resp = evaluate(req);
        assert_eq!(resp.get("decision").and_then(Value::as_str), Some("deny"));
        assert_eq!(resp.get("reason_code").and_then(Value::as_str), Some("invalid_input"));
        assert_eq!(resp.get("matched_consent_index"), Some(&Value::Null));
    }

    #[test]
    fn matches_correct_index_in_multiple_consents() {
        let req = make_request(
            "sub-7",
            "marketing",
            "2026-08-15T12:00:00Z",
            "1.0.0",
            alloc::vec![
                make_consent("essential", "granted", None, None, None),
                make_consent("analytics", "withdrawn", None, None, None),
                make_consent("marketing", "granted", None, None, None),
            ],
        );
        let resp = evaluate(req);
        assert_eq!(resp.get("decision").and_then(Value::as_str), Some("allow"));
        assert_eq!(resp.get("reason_code").and_then(Value::as_str), Some("consent_granted"));
        assert_eq!(resp.get("matched_consent_index").and_then(Value::as_f64), Some(2.0));
    }

    #[test]
    fn rejects_missing_required_fields() {
        let r1 = object(alloc::vec![
            ("requested_purpose", Value::String(String::from("p"))),
            ("now", Value::String(String::from("2026-08-15T12:00:00Z"))),
            ("policy_version", Value::String(String::from("1.0.0"))),
            ("consents", Value::Array(alloc::vec![])),
        ]);
        assert_eq!(evaluate(r1).get("reason_code").and_then(Value::as_str), Some("invalid_input"));

        let r1b = object(alloc::vec![
            ("subject_id", Value::String(String::from(""))),
            ("requested_purpose", Value::String(String::from("p"))),
            ("now", Value::String(String::from("2026-08-15T12:00:00Z"))),
            ("policy_version", Value::String(String::from("1.0.0"))),
            ("consents", Value::Array(alloc::vec![])),
        ]);
        assert_eq!(evaluate(r1b).get("reason_code").and_then(Value::as_str), Some("invalid_input"));

        let r2 = object(alloc::vec![
            ("subject_id", Value::String(String::from("s"))),
            ("now", Value::String(String::from("2026-08-15T12:00:00Z"))),
            ("policy_version", Value::String(String::from("1.0.0"))),
            ("consents", Value::Array(alloc::vec![])),
        ]);
        assert_eq!(evaluate(r2).get("reason_code").and_then(Value::as_str), Some("invalid_input"));

        let r3 = object(alloc::vec![
            ("subject_id", Value::String(String::from("s"))),
            ("requested_purpose", Value::String(String::from("p"))),
            ("now", Value::String(String::from("2026-08-15T12:00:00Z"))),
            ("consents", Value::Array(alloc::vec![])),
        ]);
        assert_eq!(evaluate(r3).get("reason_code").and_then(Value::as_str), Some("invalid_input"));

        let r4 = object(alloc::vec![
            ("subject_id", Value::String(String::from("s"))),
            ("requested_purpose", Value::String(String::from("p"))),
            ("policy_version", Value::String(String::from("1.0.0"))),
            ("consents", Value::Array(alloc::vec![])),
        ]);
        assert_eq!(evaluate(r4).get("reason_code").and_then(Value::as_str), Some("invalid_input"));

        let r5 = object(alloc::vec![
            ("subject_id", Value::String(String::from("s"))),
            ("requested_purpose", Value::String(String::from("p"))),
            ("now", Value::String(String::from("2026-08-15T12:00:00Z"))),
            ("policy_version", Value::String(String::from("1.0.0"))),
        ]);
        assert_eq!(evaluate(r5).get("reason_code").and_then(Value::as_str), Some("invalid_input"));
    }

    #[test]
    fn rejects_invalid_timestamps() {
        let bad_now = make_request("s", "p", "not-a-timestamp", "1.0.0", alloc::vec![]);
        assert_eq!(evaluate(bad_now).get("reason_code").and_then(Value::as_str), Some("invalid_input"));

        let bad_granted = make_request(
            "s",
            "p",
            "2026-08-15T12:00:00Z",
            "1.0.0",
            alloc::vec![make_consent("p", "granted", Some("bad-date"), None, None)],
        );
        assert_eq!(evaluate(bad_granted).get("reason_code").and_then(Value::as_str), Some("invalid_input"));

        let bad_expires = make_request(
            "s",
            "p",
            "2026-08-15T12:00:00Z",
            "1.0.0",
            alloc::vec![make_consent("p", "granted", None, Some("bad-date"), None)],
        );
        assert_eq!(evaluate(bad_expires).get("reason_code").and_then(Value::as_str), Some("invalid_input"));
    }

    #[test]
    fn rejects_invalid_consent_fields() {
        let bad_c1 = make_request(
            "s",
            "p",
            "2026-08-15T12:00:00Z",
            "1.0.0",
            alloc::vec![object(alloc::vec![("status", Value::String(String::from("granted")))])],
        );
        assert_eq!(evaluate(bad_c1).get("reason_code").and_then(Value::as_str), Some("invalid_input"));

        let bad_c2 = make_request(
            "s",
            "p",
            "2026-08-15T12:00:00Z",
            "1.0.0",
            alloc::vec![make_consent("p", "unknown_status", None, None, None)],
        );
        assert_eq!(evaluate(bad_c2).get("reason_code").and_then(Value::as_str), Some("invalid_input"));

        let bad_scope = make_request(
            "s",
            "p",
            "2026-08-15T12:00:00Z",
            "1.0.0",
            alloc::vec![object(alloc::vec![
                ("purpose", Value::String(String::from("p"))),
                ("status", Value::String(String::from("granted"))),
                ("scope", Value::Array(alloc::vec![Value::Number(42.0)])),
            ])],
        );
        assert_eq!(evaluate(bad_scope).get("reason_code").and_then(Value::as_str), Some("invalid_input"));

        let bad_scope_shape = make_request(
            "s",
            "p",
            "2026-08-15T12:00:00Z",
            "1.0.0",
            alloc::vec![object(alloc::vec![
                ("purpose", Value::String(String::from("p"))),
                ("status", Value::String(String::from("granted"))),
                ("scope", Value::String(String::from("not-an-array"))),
            ])],
        );
        assert_eq!(evaluate(bad_scope_shape).get("reason_code").and_then(Value::as_str), Some("invalid_input"));

        let bad_granted_type = make_request(
            "s",
            "p",
            "2026-08-15T12:00:00Z",
            "1.0.0",
            alloc::vec![object(alloc::vec![
                ("purpose", Value::String(String::from("p"))),
                ("status", Value::String(String::from("granted"))),
                ("granted_at", Value::Bool(true)),
            ])],
        );
        assert_eq!(evaluate(bad_granted_type).get("reason_code").and_then(Value::as_str), Some("invalid_input"));

        let bad_expires_type = make_request(
            "s",
            "p",
            "2026-08-15T12:00:00Z",
            "1.0.0",
            alloc::vec![object(alloc::vec![
                ("purpose", Value::String(String::from("p"))),
                ("status", Value::String(String::from("granted"))),
                ("expires_at", Value::Number(123.0)),
            ])],
        );
        assert_eq!(evaluate(bad_expires_type).get("reason_code").and_then(Value::as_str), Some("invalid_input"));
    }

    #[test]
    fn parses_various_iso8601_flavors() {
        assert!(parse_timestamp("2026-08-15T12:00:00Z").is_some());
        assert!(parse_timestamp("2026-08-15t12:00:00z").is_some());
        assert!(parse_timestamp("2026-08-15T12:00:00.123456789Z").is_some());
        assert!(parse_timestamp("2026-08-15T12:00:00,500Z").is_some());
        assert!(parse_timestamp("2026-08-15T14:00:00+02:00").is_some());
        assert!(parse_timestamp("2026-08-15T10:00:00-0200").is_some());
        assert!(parse_timestamp("2026-08-15T12:00:00+00").is_some());

        assert!(parse_timestamp("short").is_none());
        assert!(parse_timestamp("2026/08/15T12:00:00Z").is_none());
        assert!(parse_timestamp("2026-08-15 12:00:00Z").is_none());
        assert!(parse_timestamp("2026-08-15T12-00-00Z").is_none());
        assert!(parse_timestamp("2026-13-15T12:00:00Z").is_none());
        assert!(parse_timestamp("2026-00-15T12:00:00Z").is_none());
        assert!(parse_timestamp("2026-02-29T12:00:00Z").is_none());
        assert!(parse_timestamp("2024-02-29T12:00:00Z").is_some());
        assert!(parse_timestamp("2026-08-32T12:00:00Z").is_none());
        assert!(parse_timestamp("2026-08-15T24:00:00Z").is_none());
        assert!(parse_timestamp("2026-08-15T12:60:00Z").is_none());
        assert!(parse_timestamp("2026-08-15T12:00:60Z").is_none());
        assert!(parse_timestamp("2026-08-15T12:00:00").is_none());
        assert!(parse_timestamp("2026-08-15T12:00:00.Z").is_none());
        assert!(parse_timestamp("2026-08-15T12:00:00+25:00").is_none());
        assert!(parse_timestamp("2026-08-15T12:00:00+02:65").is_none());
        assert!(parse_timestamp("2026-08-15T12:00:00+020").is_none());
        assert!(parse_timestamp("2026-08-15T12:00:00+25").is_none());
        assert!(parse_timestamp("2026-08-15T12:00:00Zextra").is_none());
    }

    #[test]
    fn tests_helpers_directly() {
        assert!(is_leap_year(2000));
        assert!(!is_leap_year(1900));
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(2025));

        assert_eq!(days_in_month(2024, 0), None);
        assert_eq!(days_in_month(2024, 13), None);
        assert_eq!(days_in_month(2024, 2), Some(29));
        assert_eq!(days_in_month(2025, 2), Some(28));
        assert_eq!(days_in_month(2024, 4), Some(30));
        assert_eq!(days_in_month(2024, 1), Some(31));

        assert_eq!(parse_u32(b""), None);
        assert_eq!(parse_u32(b"abc"), None);
        assert_eq!(parse_u32(b"123"), Some(123));
        assert_eq!(parse_u32(b"99999999999999999999"), None);
    }
}
