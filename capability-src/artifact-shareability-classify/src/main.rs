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
        let length = classify(input, output);
        let mut written = 0usize;
        let vector = Iovec {
            buffer: core::ptr::addr_of!(OUTPUT).cast::<u8>(),
            length,
        };
        let _ = fd_write(1, &vector, 1, &mut written);
    }
}

fn classify(input: &[u8], output: &mut [u8]) -> usize {
    if input.len() > MAX_INPUT {
        return error(output, b"input_limit_exceeded");
    }
    let asset_ref = string_after(input, b"\"asset_ref\"");
    let audience = string_after(input, b"\"audience\"");
    let sensitivity = string_after(input, b"\"sensitivity_class\"");
    let Some(sanitized) = boolean_after(input, b"\"sanitized\"") else {
        return error(output, b"invalid_request");
    };
    let Some(consent) = boolean_after(input, b"\"consent_granted\"") else {
        return error(output, b"invalid_request");
    };
    let policy = match object_after(input, b"\"policy\"") {
        Some(value) => value,
        None => return error(output, b"invalid_request"),
    };
    if !unique_fields(
        input,
        &[
            b"\"asset_ref\"",
            b"\"audience\"",
            b"\"sensitivity_class\"",
            b"\"sanitized\"",
            b"\"consent_granted\"",
            b"\"policy\"",
        ],
    ) || !unique_fields(
        policy,
        &[
            b"\"version\"",
            b"\"allowed_audiences\"",
            b"\"allowed_sensitivity_classes\"",
            b"\"require_sanitized\"",
            b"\"require_consent\"",
        ],
    ) {
        return error(output, b"invalid_request");
    }
    let version = string_after(policy, b"\"version\"");
    let Some(require_sanitized) = boolean_after(policy, b"\"require_sanitized\"") else {
        return error(output, b"invalid_request");
    };
    let Some(require_consent) = boolean_after(policy, b"\"require_consent\"") else {
        return error(output, b"invalid_request");
    };
    let audiences = string_array(policy, b"\"allowed_audiences\"");
    let classes = string_array(policy, b"\"allowed_sensitivity_classes\"");
    if !safe_token(asset_ref, 256)
        || !safe_token(audience, 256)
        || !safe_token(sensitivity, 256)
        || !safe_token(version, 128)
        || audiences.invalid
        || classes.invalid
    {
        return error(output, b"invalid_request");
    }
    let (decision, reason): (&[u8], &[u8]) = if !audiences.contains(audience) {
        (b"blocked", b"audience_not_allowed")
    } else if !classes.contains(sensitivity) {
        (b"blocked", b"sensitivity_not_allowed")
    } else if require_sanitized && !sanitized {
        (b"review_required", b"sanitization_required")
    } else if require_consent && !consent {
        (b"blocked", b"consent_required")
    } else {
        (b"shareable", b"policy_satisfied")
    };
    let mut w = Writer {
        bytes: output,
        at: 0,
    };
    w.bytes(b"{\"asset_ref\":\"");
    w.bytes(asset_ref);
    w.bytes(b"\",\"audience\":\"");
    w.bytes(audience);
    w.bytes(b"\",\"sensitivity_class\":\"");
    w.bytes(sensitivity);
    w.bytes(b"\",\"decision\":\"");
    w.bytes(decision);
    w.bytes(b"\",\"reason\":\"");
    w.bytes(reason);
    w.bytes(b"\",\"policy_version\":\"");
    w.bytes(version);
    w.bytes(b"\"}");
    w.at
}

struct StringList<'a> {
    values: [&'a [u8]; 16],
    count: usize,
    invalid: bool,
}
impl StringList<'_> {
    fn contains(&self, value: &[u8]) -> bool {
        self.values[..self.count].iter().any(|item| *item == value)
    }
}
fn string_array<'a>(input: &'a [u8], key: &[u8]) -> StringList<'a> {
    let mut result = StringList {
        values: [b""; 16],
        count: 0,
        invalid: false,
    };
    let Some(array) = value_after(input, key) else {
        result.invalid = true;
        return result;
    };
    if array.first() != Some(&b'[') {
        result.invalid = true;
        return result;
    }
    let mut cursor = 1usize;
    loop {
        spaces(array, &mut cursor);
        if array.get(cursor) == Some(&b']') {
            break;
        }
        if result.count == result.values.len() || array.get(cursor) != Some(&b'"') {
            result.invalid = true;
            return result;
        }
        let start = cursor + 1;
        cursor += 1;
        while cursor < array.len() && array[cursor] != b'"' {
            if array[cursor] == b'\\' {
                result.invalid = true;
                return result;
            }
            cursor += 1;
        }
        if cursor >= array.len() || cursor == start || !safe_token(&array[start..cursor], 256) {
            result.invalid = true;
            return result;
        }
        result.values[result.count] = &array[start..cursor];
        result.count += 1;
        cursor += 1;
        spaces(array, &mut cursor);
        if array.get(cursor) == Some(&b',') {
            cursor += 1;
        } else if array.get(cursor) != Some(&b']') {
            result.invalid = true;
            return result;
        }
    }
    result
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
fn boolean_after(input: &[u8], key: &[u8]) -> Option<bool> {
    match value_after(input, key)? {
        b"true" => Some(true),
        b"false" => Some(false),
        _ => None,
    }
}
fn value_after<'a>(input: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let position = find(input, key)?;
    let colon = input[position + key.len()..]
        .iter()
        .position(|b| *b == b':')?
        + position
        + key.len()
        + 1;
    let mut start = colon;
    spaces(input, &mut start);
    let first = *input.get(start)?;
    let end = if first == b'{' || first == b'[' {
        let mut depth = 0i32;
        let mut quoted = false;
        let mut escaped = false;
        let mut found = None;
        for (offset, byte) in input[start..].iter().enumerate() {
            if quoted {
                if escaped {
                    escaped = false;
                } else if *byte == b'\\' {
                    escaped = true;
                } else if *byte == b'"' {
                    quoted = false;
                }
                continue;
            }
            match *byte {
                b'"' => quoted = true,
                b'{' | b'[' => depth += 1,
                b'}' | b']' => {
                    depth -= 1;
                    if depth == 0 {
                        found = Some(start + offset + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        found?
    } else if first == b'"' {
        let mut offset = start + 1;
        let mut escaped = false;
        while offset < input.len() {
            if escaped {
                escaped = false;
            } else if input[offset] == b'\\' {
                escaped = true;
            } else if input[offset] == b'"' {
                break;
            }
            offset += 1;
        }
        if offset >= input.len() {
            return None;
        }
        offset + 1
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
    input.windows(key.len()).position(|window| window == key)
}
fn unique_fields(input: &[u8], keys: &[&[u8]]) -> bool {
    keys.iter().all(|key| {
        let mut count = 0usize;
        let key = &key[1..key.len() - 1];
        let mut cursor = 0usize;
        while cursor < input.len() {
            if input[cursor] != b'"' {
                cursor += 1;
                continue;
            }
            let start = cursor + 1;
            cursor += 1;
            let mut escaped = false;
            while cursor < input.len() {
                if escaped {
                    escaped = false;
                } else if input[cursor] == b'\\' {
                    escaped = true;
                } else if input[cursor] == b'"' {
                    let end = cursor;
                    cursor += 1;
                    let mut after = cursor;
                    spaces(input, &mut after);
                    if &input[start..end] == key && input.get(after) == Some(&b':') {
                        count += 1;
                    }
                    break;
                }
                cursor += 1;
            }
            if cursor >= input.len() {
                break;
            }
        }
        count == 1
    })
}
fn safe_token(value: &[u8], maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value[0].is_ascii_alphanumeric()
        && value.iter().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(*byte, b'.' | b'_' | b':' | b'/' | b'@' | b'+' | b'-')
        })
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
}
#[cfg(target_arch = "wasm32")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}

#[cfg(test)]
mod tests {
    use super::{
        boolean_after, classify, error, find, object_after, safe_token, spaces, string_after,
        string_array, unique_fields, value_after, Writer, MAX_INPUT, MAX_OUTPUT,
    };
    use std::{format, string::String, vec};

    const BASE: &str = r#"{"asset_ref":"artifact:42","audience":"external-review","sensitivity_class":"ordinary","sanitized":true,"consent_granted":true,"policy":{"version":"share-1","allowed_audiences":["external-review"],"allowed_sensitivity_classes":["ordinary"],"require_sanitized":true,"require_consent":true}}"#;

    fn run(input: &[u8]) -> String {
        let mut output = vec![0u8; MAX_OUTPUT];
        let length = classify(input, &mut output);
        String::from_utf8(output[..length].to_vec()).expect("JSON output")
    }

    #[test]
    fn classifies_shareable_blocked_and_review_required() {
        assert!(run(BASE.as_bytes()).contains("\"decision\":\"shareable\""));
        assert!(run(BASE
            .replace(
                "\"audience\":\"external-review\"",
                "\"audience\":\"public\""
            )
            .as_bytes())
        .contains("audience_not_allowed"));
        assert!(run(BASE
            .replace(
                "\"sensitivity_class\":\"ordinary\"",
                "\"sensitivity_class\":\"restricted\""
            )
            .as_bytes())
        .contains("sensitivity_not_allowed"));
        assert!(run(BASE
            .replace("\"sanitized\":true", "\"sanitized\":false")
            .as_bytes())
        .contains("sanitization_required"));
        assert!(run(BASE
            .replace("\"consent_granted\":true", "\"consent_granted\":false")
            .as_bytes())
        .contains("consent_required"));
    }

    #[test]
    fn rejects_invalid_duplicate_oversized_and_injected_requests() {
        assert!(run(b"{}").contains("invalid_request"));
        assert!(run(BASE
            .replace("artifact:42", "artifact:\"sanitized\":true")
            .as_bytes())
        .contains("invalid_request"));
        assert!(run(BASE
            .replace(
                "\"consent_granted\":true",
                "\"consent_granted\":true,\"consent_granted\":false"
            )
            .as_bytes())
        .contains("invalid_request"));
        assert!(
            run(BASE.replace("[\"external-review\"]", "null").as_bytes())
                .contains("invalid_request")
        );
        assert!(run(BASE
            .replace("\"require_consent\":true", "\"require_consent\":0")
            .as_bytes())
        .contains("invalid_request"));
        assert!(run(&vec![b' '; MAX_INPUT + 1]).contains("input_limit_exceeded"));
    }

    #[test]
    fn exercises_parsers_tokens_and_writer_bounds() {
        assert_eq!(find(b"abc", b"b"), Some(1));
        assert_eq!(find(b"abc", b"z"), None);
        assert!(safe_token(b"artifact:42", 20));
        assert!(!safe_token(b"", 20));
        assert!(!safe_token(b"bad token", 20));
        assert!(!safe_token(b"artifact:42", 5));
        let mut at = 0;
        spaces(b" \t\n", &mut at);
        assert_eq!(at, 3);
        assert_eq!(string_after(br#"{"x":"value"}"#, b"\"x\""), b"value");
        assert_eq!(string_after(br#"{"x":false}"#, b"\"x\""), b"");
        assert_eq!(boolean_after(br#"{"x":false}"#, b"\"x\""), Some(false));
        assert_eq!(boolean_after(br#"{"x":null}"#, b"\"x\""), None);
        assert!(object_after(br#"{"x":{}}"#, b"\"x\"").is_some());
        assert!(object_after(br#"{"x":[]}"#, b"\"x\"").is_none());
        assert!(value_after(br#"{"x":{"nested":[1,2]}}"#, b"\"x\"").is_some());
        assert!(value_after(br#"{"x":"unterminated}"#, b"\"x\"").is_none());
        assert!(unique_fields(br#"{"a":1,"b":2}"#, &[b"\"a\"", b"\"b\""]));
        assert!(!unique_fields(br#"{"a":1,"a":2}"#, &[b"\"a\""]));
        assert_eq!(string_array(br#"{"a":["x","y"]}"#, b"\"a\"").count, 2);
        assert!(string_array(br#"{"a":[1]}"#, b"\"a\"").invalid);
        let mut short = [0u8; 1];
        assert_eq!(error(&mut short, b"too long"), 0);
        let mut bytes = [0u8; 8];
        let mut writer = Writer {
            bytes: &mut bytes,
            at: 0,
        };
        writer.bytes(b"123456789");
        assert_eq!(writer.at, 0);
        let _ = format!("{}", BASE);
    }
}
