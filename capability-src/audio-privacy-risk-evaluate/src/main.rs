#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#[cfg(test)]
extern crate std;

const MAX_INPUT: usize = 8192;
const MAX_OUTPUT: usize = 2048;
#[repr(C)]
#[cfg(target_arch = "wasm32")]
struct Iovec {
    buffer: *const u8,
    length: usize,
}
#[repr(C)]
#[cfg(target_arch = "wasm32")]
struct IovecMut {
    buffer: *mut u8,
    length: usize,
}
#[link(wasm_import_module = "wasi_snapshot_preview1")]
#[cfg(target_arch = "wasm32")]
unsafe extern "C" {
    fn fd_read(fd: u32, vectors: *const IovecMut, count: usize, read: *mut usize) -> u32;
    fn fd_write(fd: u32, vectors: *const Iovec, count: usize, written: *mut usize) -> u32;
}
#[cfg(target_arch = "wasm32")]
static mut INPUT: [u8; MAX_INPUT + 1] = [0; MAX_INPUT + 1];
#[cfg(target_arch = "wasm32")]
static mut OUTPUT: [u8; MAX_OUTPUT] = [0; MAX_OUTPUT];

#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    unsafe {
        let mut total = 0usize;
        let input_ptr = core::ptr::addr_of_mut!(INPUT).cast::<u8>();
        while total < MAX_INPUT + 1 {
            let mut count = 0usize;
            let vector = IovecMut {
                buffer: input_ptr.add(total),
                length: MAX_INPUT + 1 - total,
            };
            if fd_read(0, &vector, 1, &mut count) != 0 || count == 0 {
                break;
            }
            total += count;
        }
        let input = core::slice::from_raw_parts(input_ptr, total);
        let output = core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(OUTPUT).cast::<u8>(),
            MAX_OUTPUT,
        );
        let length = evaluate(input, output);
        let mut written = 0usize;
        let vector = Iovec {
            buffer: core::ptr::addr_of!(OUTPUT).cast::<u8>(),
            length,
        };
        let _ = fd_write(1, &vector, 1, &mut written);
    }
}

fn evaluate(input: &[u8], output: &mut [u8]) -> usize {
    if input.len() > MAX_INPUT {
        return error(output, b"input_limit_exceeded");
    }
    let subject_ref = string_after(input, b"\"subject_ref\"");
    let policy = match object_after(input, b"\"policy\"") {
        Some(value) => value,
        None => return error(output, b"invalid_request"),
    };
    let version = string_after(policy, b"\"version\"");
    let Some(risk) = unsigned_after(input, b"\"risk_score_millis\"") else {
        return error(output, b"invalid_request");
    };
    let Some(complete) = boolean_after(input, b"\"evidence_complete\"") else {
        return error(output, b"invalid_request");
    };
    let Some(threshold) = unsigned_after(policy, b"\"maximum_risk_score_millis\"") else {
        return error(output, b"invalid_request");
    };
    let incomplete_action = string_after(policy, b"\"incomplete_evidence_action\"");
    let elevated_action = string_after(policy, b"\"elevated_risk_action\"");
    if subject_ref.is_empty()
        || version.is_empty()
        || risk > 1000
        || threshold > 1000
        || !valid_action(incomplete_action)
        || !valid_action(elevated_action)
    {
        return error(output, b"invalid_request");
    }
    let (decision, reason): (&[u8], &[u8]) = if !complete {
        (incomplete_action, b"evidence_incomplete")
    } else if risk > threshold {
        (elevated_action, b"risk_exceeds_threshold")
    } else {
        (b"allow", b"risk_within_threshold")
    };
    let mut w = Writer {
        bytes: output,
        at: 0,
    };
    w.bytes(b"{\"subject_ref\":\"");
    w.bytes(subject_ref);
    w.bytes(b"\",\"risk_score_millis\":");
    w.number(risk);
    w.bytes(b",\"evidence_complete\":");
    w.bytes(if complete { b"true" } else { b"false" });
    w.bytes(b",\"decision\":\"");
    w.bytes(decision);
    w.bytes(b"\",\"reason\":\"");
    w.bytes(reason);
    w.bytes(b"\",\"policy_version\":\"");
    w.bytes(version);
    w.bytes(b"\"}");
    w.at
}
fn valid_action(action: &[u8]) -> bool {
    matches!(action, b"allow" | b"deny" | b"review_required")
}
fn object_after<'a>(input: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let value = value_after(input, key)?;
    if value.first() == Some(&b'{') {
        Some(value)
    } else {
        None
    }
}
fn string_after<'a>(input: &'a [u8], key: &[u8]) -> &'a [u8] {
    let Some(value) = value_after(input, key) else {
        return b"";
    };
    if value.len() < 2 || value[0] != b'"' || value[value.len() - 1] != b'"' {
        return b"";
    }
    &value[1..value.len() - 1]
}
fn unsigned_after(input: &[u8], key: &[u8]) -> Option<u32> {
    let value = value_after(input, key)?;
    if value.is_empty() {
        return None;
    }
    let mut result = 0u32;
    for byte in value {
        if !byte.is_ascii_digit() {
            return None;
        }
        result = result.checked_mul(10)?.checked_add((byte - b'0') as u32)?;
    }
    Some(result)
}
fn boolean_after(input: &[u8], key: &[u8]) -> Option<bool> {
    let value = value_after(input, key)?;
    match value {
        b"true" => Some(true),
        b"false" => Some(false),
        _ => None,
    }
}
fn value_after<'a>(input: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let pos = find(input, key)?;
    let colon = input[pos + key.len()..].iter().position(|b| *b == b':')? + pos + key.len() + 1;
    let mut start = colon;
    spaces(input, &mut start);
    let first = *input.get(start)?;
    let end = if first == b'{' {
        let mut depth = 0i32;
        let mut quote = false;
        let mut escaped = false;
        let mut found = None;
        for (index, byte) in input[start..].iter().enumerate() {
            if quote {
                if escaped {
                    escaped = false;
                } else if *byte == b'\\' {
                    escaped = true;
                } else if *byte == b'"' {
                    quote = false;
                }
                continue;
            }
            match *byte {
                b'"' => quote = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        found = Some(start + index + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        found?
    } else if first == b'"' {
        let mut index = start + 1;
        let mut escaped = false;
        while index < input.len() {
            if escaped {
                escaped = false;
            } else if input[index] == b'\\' {
                escaped = true;
            } else if input[index] == b'"' {
                break;
            }
            index += 1;
        }
        if index >= input.len() {
            return None;
        }
        index + 1
    } else {
        input[start..]
            .iter()
            .position(|b| matches!(*b, b',' | b'}' | b']' | b' ' | b'\n' | b'\r' | b'\t'))
            .map(|n| start + n)
            .unwrap_or(input.len())
    };
    Some(&input[start..end])
}
fn find(input: &[u8], key: &[u8]) -> Option<usize> {
    input.windows(key.len()).position(|part| part == key)
}
fn spaces(input: &[u8], at: &mut usize) {
    while input
        .get(*at)
        .is_some_and(|b| matches!(*b, b' ' | b'\n' | b'\r' | b'\t'))
    {
        *at += 1;
    }
}
fn error(output: &mut [u8], code: &[u8]) -> usize {
    let mut w = Writer {
        bytes: output,
        at: 0,
    };
    w.bytes(b"{\"result_class\":\"");
    w.bytes(code);
    w.bytes(b"\"}");
    w.at
}
struct Writer<'a> {
    bytes: &'a mut [u8],
    at: usize,
}
impl Writer<'_> {
    fn bytes(&mut self, value: &[u8]) {
        let end = self.at.saturating_add(value.len());
        if end <= self.bytes.len() {
            self.bytes[self.at..end].copy_from_slice(value);
            self.at = end;
        } else {
            self.at = 0;
        }
    }
    fn number(&mut self, mut value: u32) {
        let mut digits = [0u8; 10];
        let mut n = 0;
        loop {
            digits[n] = b'0' + (value % 10) as u8;
            n += 1;
            value /= 10;
            if value == 0 {
                break;
            }
        }
        while n > 0 {
            n -= 1;
            self.bytes(&digits[n..=n]);
        }
    }
}
#[cfg(target_arch = "wasm32")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}

#[cfg(test)]
mod tests {
    use super::{
        boolean_after, error, evaluate, find, object_after, spaces, string_after, unsigned_after,
        valid_action, value_after, Writer, MAX_INPUT, MAX_OUTPUT,
    };
    use std::{format, string::String, vec};

    const BASE: &str = r#"{"subject_ref":"asset:42","risk_score_millis":120,"evidence_complete":true,"policy":{"version":"privacy-1","maximum_risk_score_millis":300,"incomplete_evidence_action":"review_required","elevated_risk_action":"deny"}}"#;

    fn run(input: &[u8]) -> String {
        let mut output = vec![0u8; MAX_OUTPUT];
        let length = evaluate(input, &mut output);
        String::from_utf8(output[..length].to_vec()).expect("JSON output")
    }

    #[test]
    fn evaluates_configured_allow_deny_and_review_actions() {
        assert!(run(BASE.as_bytes()).contains("risk_within_threshold"));
        assert!(run(BASE.replace("120", "301").as_bytes()).contains("risk_exceeds_threshold"));
        assert!(run(BASE
            .replace("\"evidence_complete\":true", "\"evidence_complete\":false")
            .as_bytes())
        .contains("evidence_incomplete"));
        let allow = BASE
            .replace("\"evidence_complete\":true", "\"evidence_complete\":false")
            .replace("review_required", "allow");
        assert!(run(allow.as_bytes()).contains("\"decision\":\"allow\""));
    }

    #[test]
    fn rejects_invalid_policy_evidence_and_resource_bounds() {
        assert!(run(b"{}").contains("invalid_request"));
        assert!(run(BASE.replace("asset:42", "").as_bytes()).contains("invalid_request"));
        assert!(run(BASE.replace("120", "1001").as_bytes()).contains("invalid_request"));
        assert!(run(BASE.replace("300", "1001").as_bytes()).contains("invalid_request"));
        assert!(
            run(BASE.replace("review_required", "maybe").as_bytes()).contains("invalid_request")
        );
        assert!(run(BASE
            .replace("\"evidence_complete\":true", "\"evidence_complete\":null")
            .as_bytes())
        .contains("invalid_request"));
        assert!(run(&vec![b' '; MAX_INPUT + 1]).contains("input_limit_exceeded"));
    }

    #[test]
    fn exercises_field_readers_and_output_bounds() {
        assert!(valid_action(b"allow"));
        assert!(valid_action(b"deny"));
        assert!(valid_action(b"review_required"));
        assert!(!valid_action(b"maybe"));
        assert_eq!(find(b"abc", b"b"), Some(1));
        assert_eq!(find(b"abc", b"z"), None);
        let mut at = 0;
        spaces(b" \t\n", &mut at);
        assert_eq!(at, 3);
        assert!(object_after(br#"{"p":{}}"#, b"\"p\"").is_some());
        assert!(object_after(br#"{"p":[]}"#, b"\"p\"").is_none());
        assert_eq!(string_after(br#"{"s":"value"}"#, b"\"s\""), b"value");
        assert_eq!(unsigned_after(br#"{"n":42}"#, b"\"n\""), Some(42));
        assert_eq!(unsigned_after(br#"{"n":-1}"#, b"\"n\""), None);
        assert_eq!(unsigned_after(br#"{"n":999999999999}"#, b"\"n\""), None);
        assert_eq!(boolean_after(br#"{"b":false}"#, b"\"b\""), Some(false));
        assert_eq!(boolean_after(br#"{"b":0}"#, b"\"b\""), None);
        assert!(value_after(br#"{"p":{"nested":"x}"}}"#, b"\"p\"").is_some());
        assert!(value_after(br#"{"p":"unterminated}"#, b"\"p\"").is_none());
        let mut short = [0u8; 1];
        assert_eq!(error(&mut short, b"too long"), 0);
        let mut bytes = [0u8; 4];
        let mut writer = Writer {
            bytes: &mut bytes,
            at: 0,
        };
        writer.number(123456);
        assert_eq!(writer.at, 1);
        assert_eq!(writer.bytes[0], b'6');
        let _ = format!("{}", BASE);
    }
}
