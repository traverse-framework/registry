#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#[cfg(test)]
extern crate std;

const MAX_INPUT: usize = 32_768;
const MAX_OUTPUT: usize = 32_768;
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
        let length = plan(input, output);
        let mut written = 0usize;
        let vector = Iovec {
            buffer: core::ptr::addr_of!(OUTPUT).cast::<u8>(),
            length,
        };
        let _ = fd_write(1, &vector, 1, &mut written);
    }
}

fn plan(input: &[u8], output: &mut [u8]) -> usize {
    if input.len() > MAX_INPUT {
        return error(output, b"input_limit_exceeded");
    }
    let event_id = string_after(input, b"\"event_id\"");
    let event_type = string_after(input, b"\"event_type\"");
    let schema_version = string_after(input, b"\"schema_version\"");
    let aggregate_ref = string_after(input, b"\"aggregate_ref\"");
    let payload_ref = string_after(input, b"\"payload_ref\"");
    let occurred_at = string_after(input, b"\"occurred_at\"");
    let idempotency_key = string_after(input, b"\"idempotency_key\"");
    let causation_refs = array_after(input, b"\"causation_refs\"");
    if event_id.is_empty()
        || event_type.is_empty()
        || schema_version.is_empty()
        || aggregate_ref.is_empty()
        || payload_ref.is_empty()
        || occurred_at.is_empty()
        || idempotency_key.is_empty()
    {
        return error(output, b"invalid_request");
    }
    let Some(causation_refs) = causation_refs else {
        return error(output, b"invalid_request");
    };
    if !valid_string_array(causation_refs, 16) {
        return error(output, b"invalid_causation_refs");
    }
    let mut w = Writer {
        bytes: output,
        at: 0,
    };
    w.bytes(b"{\"connector_id\":\"traverse.event-store\",\"operation\":\"append\",\"event\":{\"event_id\":\"");
    w.json(event_id);
    w.bytes(b"\",\"event_type\":\"");
    w.json(event_type);
    w.bytes(b"\",\"schema_version\":\"");
    w.json(schema_version);
    w.bytes(b"\",\"aggregate_ref\":\"");
    w.json(aggregate_ref);
    w.bytes(b"\",\"payload_ref\":\"");
    w.json(payload_ref);
    w.bytes(b"\",\"occurred_at\":\"");
    w.json(occurred_at);
    w.bytes(b"\",\"causation_refs\":");
    w.bytes(causation_refs);
    w.bytes(b"},\"idempotency_key\":\"");
    w.json(idempotency_key);
    w.bytes(b"\",\"result_class\":\"planned\"}");
    w.at
}

fn valid_string_array(array: &[u8], maximum: usize) -> bool {
    if array.first() != Some(&b'[') || array.last() != Some(&b']') {
        return false;
    }
    let mut at = 1usize;
    let mut count = 0usize;
    loop {
        spaces(array, &mut at);
        if array.get(at) == Some(&b']') {
            return true;
        }
        if count == maximum || array.get(at) != Some(&b'"') {
            return false;
        }
        at += 1;
        let mut escaped = false;
        let start = at;
        while at < array.len() {
            let byte = array[at];
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                break;
            }
            at += 1;
        }
        if at >= array.len() || at == start {
            return false;
        }
        at += 1;
        count += 1;
        spaces(array, &mut at);
        if array.get(at) == Some(&b',') {
            at += 1;
            continue;
        }
        if array.get(at) == Some(&b']') {
            return true;
        }
        return false;
    }
}
fn array_after<'a>(input: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let value = value_after(input, key)?;
    if value.first() == Some(&b'[') {
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
fn value_after<'a>(input: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let pos = find(input, key)?;
    let colon = input[pos + key.len()..].iter().position(|b| *b == b':')? + pos + key.len() + 1;
    let mut start = colon;
    spaces(input, &mut start);
    let first = *input.get(start)?;
    let end = if first == b'{' || first == b'[' {
        let mut depth = 0i32;
        let mut quote = false;
        let mut escape = false;
        let mut found = None;
        for (i, byte) in input[start..].iter().enumerate() {
            if quote {
                if escape {
                    escape = false;
                } else if *byte == b'\\' {
                    escape = true;
                } else if *byte == b'"' {
                    quote = false;
                }
                continue;
            }
            match *byte {
                b'"' => quote = true,
                b'{' | b'[' => depth += 1,
                b'}' | b']' => {
                    depth -= 1;
                    if depth == 0 {
                        found = Some(start + i + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        found?
    } else if first == b'"' {
        let mut i = start + 1;
        let mut escape = false;
        while i < input.len() {
            if escape {
                escape = false;
            } else if input[i] == b'\\' {
                escape = true;
            } else if input[i] == b'"' {
                break;
            }
            i += 1;
        }
        if i >= input.len() {
            return None;
        }
        i + 1
    } else {
        return None;
    };
    Some(&input[start..end])
}
fn find(input: &[u8], key: &[u8]) -> Option<usize> {
    input.windows(key.len()).position(|window| window == key)
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
    fn json(&mut self, value: &[u8]) {
        // `string_after` returns the already-escaped JSON string contents.
        self.bytes(value);
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
        array_after, error, find, plan, spaces, string_after, valid_string_array, value_after,
        Writer, MAX_INPUT, MAX_OUTPUT,
    };
    use std::{format, string::String, vec};

    const BASE: &str = r#"{"event_id":"event:42","event_type":"record.transitioned","schema_version":"1.0.0","aggregate_ref":"record:17","payload_ref":"artifact:sha256:abc","occurred_at":"2026-09-14T12:00:00Z","causation_refs":["event:40","event:41"],"idempotency_key":"event:42"}"#;

    fn run(input: &[u8]) -> String {
        let mut output = vec![0u8; MAX_OUTPUT];
        let length = plan(input, &mut output);
        String::from_utf8(output[..length].to_vec()).expect("JSON output")
    }

    #[test]
    fn creates_append_plan_preserving_envelope_and_idempotency() {
        let output = run(BASE.as_bytes());
        assert!(output.contains("traverse.event-store"));
        assert!(output.contains("\"operation\":\"append\""));
        assert!(output.contains("\"idempotency_key\":\"event:42\""));
        assert!(output.contains("\"causation_refs\":[\"event:40\",\"event:41\"]"));
        assert!(
            run(BASE.replace("event:42", r#"event:\"quoted\""#).as_bytes())
                .contains(r#"event:\"quoted\""#)
        );
    }

    #[test]
    fn rejects_missing_fields_invalid_causation_and_input_overflow() {
        assert!(run(b"{}").contains("invalid_request"));
        assert!(run(BASE
            .replace("\"event_id\":\"event:42\"", "\"event_id\":\"\"")
            .as_bytes())
        .contains("invalid_request"));
        assert!(run(BASE
            .replace("[\"event:40\",\"event:41\"]", "null")
            .as_bytes())
        .contains("invalid_request"));
        assert!(run(BASE
            .replace("[\"event:40\",\"event:41\"]", "[3]")
            .as_bytes())
        .contains("invalid_causation_refs"));
        let refs = (0..17).map(|_| "\"e\"").collect::<Vec<_>>().join(",");
        let too_many = BASE.replace("[\"event:40\",\"event:41\"]", &format!("[{refs}]"));
        assert!(run(too_many.as_bytes()).contains("invalid_causation_refs"));
        assert!(run(&vec![b' '; MAX_INPUT + 1]).contains("input_limit_exceeded"));
    }

    #[test]
    fn exercises_json_readers_array_validation_and_writer_bounds() {
        assert_eq!(find(b"abc", b"b"), Some(1));
        assert_eq!(find(b"abc", b"z"), None);
        let mut at = 0;
        spaces(b" \t\n", &mut at);
        assert_eq!(at, 3);
        assert_eq!(string_after(br#"{"s":"value"}"#, b"\"s\""), b"value");
        assert_eq!(string_after(br#"{"n":2}"#, b"\"n\""), b"");
        assert!(array_after(br#"{"a":[]}"#, b"\"a\"").is_some());
        assert!(array_after(br#"{"a":{}}"#, b"\"a\"").is_none());
        assert!(value_after(br#"{"n":2}"#, b"\"n\"").is_none());
        assert!(value_after(br#"{"a":{"nested":[1]}}"#, b"\"a\"").is_some());
        assert!(valid_string_array(br#"["a","b"]"#, 2));
        assert!(valid_string_array(b"[]", 0));
        assert!(!valid_string_array(b"{}", 2));
        assert!(!valid_string_array(br#"["a"]"#, 0));
        assert!(!valid_string_array(br#"[""]"#, 2));
        assert!(!valid_string_array(br#"["a" "b"]"#, 2));
        assert!(!valid_string_array(b"[\"unfinished", 2));
        let mut short = [0u8; 1];
        assert_eq!(error(&mut short, b"long"), 0);
        let mut bytes = [0u8; 2];
        let mut writer = Writer {
            bytes: &mut bytes,
            at: 0,
        };
        writer.bytes(b"too long");
        assert_eq!(writer.at, 0);
        let _ = format!("{}", BASE);
    }
}
