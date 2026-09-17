// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 Callweave contributors

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

#[repr(C)]
struct IoVec {
    buffer: *const u8,
    length: usize,
}

#[repr(C)]
struct IoVecMut {
    buffer: *mut u8,
    length: usize,
}

#[link(wasm_import_module = "wasi_snapshot_preview1")]
unsafe extern "C" {
    fn fd_read(fd: u32, vectors: *const IoVecMut, count: usize, read: *mut usize) -> u32;
    fn fd_write(fd: u32, vectors: *const IoVec, count: usize, written: *mut usize) -> u32;
}

#[cfg(not(test))]
#[link(wasm_import_module = "traverse_host")]
unsafe extern "C" {
    fn connector_invoke(request_ptr: i32, request_len: i32, response_ptr: i32, response_capacity: i32) -> i32;
}

#[cfg(test)]
use std::sync::atomic::{AtomicU8, Ordering};

#[cfg(test)]
static TEST_CONNECTOR_MODE: AtomicU8 = AtomicU8::new(0);

#[cfg(test)]
#[no_mangle]
unsafe extern "C" fn connector_invoke(_: i32, _: i32, response_ptr: i32, response_capacity: i32) -> i32 {
    let _ = (response_ptr, response_capacity);
    0
}

static mut INPUT: [u8; 8192] = [0; 8192];
static mut REQUEST: [u8; 8192] = [0; 8192];
static mut RESPONSE: [u8; 4096] = [0; 4096];
static mut OUTPUT: [u8; 4096] = [0; 4096];
const INPUT_CAPACITY: usize = 8192;
const REQUEST_CAPACITY: usize = 8192;
const RESPONSE_CAPACITY: usize = 4096;
const OUTPUT_CAPACITY: usize = 4096;

#[unsafe(no_mangle)]
#[cfg(not(test))]
pub extern "C" fn _start() {
    unsafe {
        let input_ptr: *mut [u8; INPUT_CAPACITY] = &raw mut INPUT;
        let request_ptr: *mut [u8; REQUEST_CAPACITY] = &raw mut REQUEST;
        let response_ptr: *mut [u8; RESPONSE_CAPACITY] = &raw mut RESPONSE;
        let output_ptr: *mut [u8; OUTPUT_CAPACITY] = &raw mut OUTPUT;
        let input = core::slice::from_raw_parts_mut(input_ptr.cast::<u8>(), INPUT_CAPACITY);
        let request = core::slice::from_raw_parts_mut(request_ptr.cast::<u8>(), REQUEST_CAPACITY);
        let response = core::slice::from_raw_parts_mut(response_ptr.cast::<u8>(), RESPONSE_CAPACITY);
        let output = core::slice::from_raw_parts_mut(output_ptr.cast::<u8>(), OUTPUT_CAPACITY);
        let mut total = 0usize;
        loop {
            if total == input.len() {
                break;
            }
            let mut read = 0usize;
            let vector = IoVecMut {
                buffer: input.as_mut_ptr().add(total),
                length: input.len() - total,
            };
            if fd_read(0, &vector, 1, &mut read) != 0 || read == 0 {
                break;
            }
            total += read;
        }

        let oversized = if total == input.len() {
            let mut extra = [0u8; 1];
            let mut read = 0usize;
            let vector = IoVecMut { buffer: extra.as_mut_ptr(), length: extra.len() };
            fd_read(0, &vector, 1, &mut read) != 0 || read > 0
        } else {
            false
        };
        let output_length = if oversized {
            unavailable(output, b"invalid_request")
        } else {
            apply_transition(&input[..total], request, response, output)
        };
        let mut written = 0usize;
        let vector = IoVec {
            buffer: output.as_ptr(),
            length: output_length,
        };
        let _ = fd_write(1, &vector, 1, &mut written);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_required_fields() {
        let mut request = [0u8; 8192];
        let mut response = [0u8; 4096];
        let mut output = [0u8; 4096];
        assert_eq!(unsafe { connector_invoke(0, 0, 0, 0) }, 0);
        let length = apply_transition(b"{}", &mut request, &mut response, &mut output);
        assert_eq!(&output[..length], b"{\"result_ref\":\"\",\"version\":0,\"replay\":false,\"result_class\":\"invalid_request\"}");
    }

    #[test]
    fn exercises_validation_and_encoding_helpers() {
        let mut request = [0u8; 8192];
        let mut response = [0u8; 4096];
        let mut output = [0u8; 4096];
        assert_eq!(skip(b" \n\tx"), b"x");
        assert!(value_after(b"[]", b"x").is_none());
        assert!(value_after(br#"{"record_refs":[],"transition":{},"idempotency_key":"k","expected_version":12}"#, b"\"record_refs\"").is_some());
        assert!(value_after(br#"{"x":1,"x":2}"#, b"\"x\"").is_none());
        assert!(json_value_end(br#"{"x":1}"#).is_some());
        assert!(json_value_end(br#"[1]"#).is_some());
        assert!(json_value_end(br#""x""#).is_some());
        assert!(balanced_end(br#"{"x":"[\\\"]"}"#).is_some());
        assert!(balanced_end(br#"{"x":"escaped\\\"quote"}"#).is_some());
        assert!(balanced_end(b"{").is_none());
        assert!(balanced_end(b"}").is_none());
        assert!(string_end(b"\"x\"").is_some());
        assert!(string_end(b"\"x").is_none());
        assert_eq!(string_after(br#"{"k":"value"}"#, b"\"k\""), b"value");
        assert_eq!(string_after(br#"{"k":1}"#, b"\"k\""), b"");
        assert_eq!(int_value_after(br#"{"n":123}"#, b"\"n\""), Some(123));
        assert!(int_value_after(br#"{"n":"x"}"#, b"\"n\"").is_none());
        assert!(int_value_after(br#"{"n":999999999999999999999}"#, b"\"n\"").is_none());
        assert_eq!(copy(&mut output, 0, b"x"), 1);
        let mut at = 1;
        assert!(append(&mut output, &mut at, b"y"));
        assert!(append_json_string(&mut output, &mut at, b"ok"));
        assert!(append_i32(&mut output, &mut at, 42));
        assert!(append_i32(&mut output, &mut at, 0));
        let mut tiny = [0u8; 1];
        let mut tiny_at = 1;
        assert!(!append(&mut tiny, &mut tiny_at, b"x"));
        assert!(!append_json_string(&mut tiny, &mut tiny_at, b"x"));
        assert!(!append_i32(&mut tiny, &mut tiny_at, 7));
        assert_eq!(unavailable(&mut tiny, b"x"), 0);
        assert!(unavailable(&mut output, b"invalid_request") > 0);
        let input = br#"{"record_refs":[],"transition":{},"idempotency_key":"k","expected_version":12}"#;
        let n = apply_transition(input, &mut request, &mut response, &mut output);
        assert!(core::str::from_utf8(&output[..n]).unwrap().contains("\"result_ref\":\"r\""));
        TEST_CONNECTOR_MODE.store(1, Ordering::Relaxed);
        let n = apply_transition(input, &mut request, &mut response, &mut output);
        assert!(core::str::from_utf8(&output[..n]).unwrap().contains("connector_unavailable"));
        TEST_CONNECTOR_MODE.store(2, Ordering::Relaxed);
        let n = apply_transition(input, &mut request, &mut response, &mut output);
        assert!(core::str::from_utf8(&output[..n]).unwrap().contains("connector_unavailable"));
        TEST_CONNECTOR_MODE.store(0, Ordering::Relaxed);
        for invalid in [br#"{"record_refs":[]}"# as &[u8], br#"{"record_refs":[],"transition":{}}"#, br#"{"record_refs":{},"transition":{},"idempotency_key":"k"}"#, br#"{"record_refs":[],"transition":{},"idempotency_key":1}"#] {
            let n = apply_transition(invalid, &mut request, &mut response, &mut output);
            assert!(core::str::from_utf8(&output[..n]).unwrap().contains("invalid_request"));
        }
        for malformed in [b"{\"record_refs\":[],\"transition\":{},\"idempotency_key\":\"k\" tail".as_slice(), b"{\"record_refs\":[],\"transition\":{},\"idempotency_key\":\"k\"}".as_slice()] {
            assert!(value_after(malformed, b"\"record_refs\"").is_none() || !malformed.ends_with(b"tail"));
        }
        let mut tiny_request = [0u8; 8];
        let n = apply_transition(input, &mut tiny_request, &mut response, &mut output);
        assert!(n > 0);
    }
}

fn apply_transition(input: &[u8], request: &mut [u8], response: &mut [u8], output: &mut [u8]) -> usize {
    let Some(record_refs) = value_after(input, b"\"record_refs\"") else {
        return unavailable(output, b"invalid_request");
    };
    let Some(transition) = value_after(input, b"\"transition\"") else {
        return unavailable(output, b"invalid_request");
    };
    let idempotency_key = string_after(input, b"\"idempotency_key\"");
    if record_refs.first() != Some(&b'[') || transition.first() != Some(&b'{') || idempotency_key.is_empty() {
        return unavailable(output, b"invalid_request");
    }

    let mut at = copy(request, 0, b"{\"abi_version\":\"1.0.0\",\"connector_id\":\"traverse.state-store\",\"operation\":\"append_transition\",\"payload\":{\"record_refs\":");
    if at == 0 || !append(request, &mut at, record_refs)
        || !append(request, &mut at, b",\"transition\":")
        || !append(request, &mut at, transition)
    {
        return unavailable(output, b"invalid_request");
    }
    if let Some(version) = int_value_after(input, b"\"expected_version\"") {
        if !append(request, &mut at, b",\"expected_version\":")
            || !append_i32(request, &mut at, version)
        {
            return unavailable(output, b"invalid_request");
        }
    }
    if !append(request, &mut at, b",\"idempotency_key\":\"")
        || !append_json_string(request, &mut at, idempotency_key)
        || !append(request, &mut at, b"\"}}")
    {
        return unavailable(output, b"invalid_request");
    }
    if at == 0 || at > request.len() || at > i32::MAX as usize {
        return unavailable(output, b"invalid_request");
    }

    #[cfg(test)]
    let received = {
        let mode = TEST_CONNECTOR_MODE.load(Ordering::Relaxed);
        if mode == 2 {
            0
        } else {
            let body = if mode == 1 { b"{}" as &[u8] } else { b"{\"payload\":{\"result_ref\":\"r\",\"version\":1,\"replay\":false}}" };
            response[..body.len()].copy_from_slice(body);
            body.len() as i32
        }
    };
    #[cfg(not(test))]
    let received = unsafe {
        connector_invoke(
            request.as_ptr() as usize as i32,
            at as i32,
            response.as_mut_ptr() as usize as i32,
            response.len() as i32,
        )
    };
    if received <= 0 || received as usize > response.len() {
        return unavailable(output, b"connector_unavailable");
    }

    let Some(payload) = value_after(&response[..received as usize], b"\"payload\"") else {
        return unavailable(output, b"connector_unavailable");
    };
    if payload.first() != Some(&b'{') {
        return unavailable(output, b"connector_unavailable");
    }
    copy(output, 0, payload)
}

fn unavailable(output: &mut [u8], result_class: &[u8]) -> usize {
    let mut at = 0usize;
    at = copy(output, at, b"{\"result_ref\":\"\",\"version\":0,\"replay\":false,\"result_class\":\"");
    at = copy(output, at, result_class);
    copy(output, at, b"\"}")
}

fn skip(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(|byte| matches!(*byte, b' ' | b'\n' | b'\r' | b'\t')) {
        value = &value[1..];
    }
    value
}

fn value_after<'a>(value: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let object = skip(value);
    if object.first() != Some(&b'{') {
        return None;
    }
    let mut cursor = 1usize;
    let mut found = None;
    loop {
        let remaining = skip(&object[cursor..]);
        cursor = object.len() - remaining.len();
        if remaining.first() == Some(&b'}') {
            cursor += 1;
            return if found.is_some() && skip(&object[cursor..]).is_empty() {
                found
            } else {
                None
            };
        }
        if remaining.first() != Some(&b'"') {
            return None;
        }

        let key_end = string_end(remaining)?;
        let key_token = &remaining[..=key_end];
        cursor += key_end + 1;
        let remaining = skip(&object[cursor..]);
        cursor = object.len() - remaining.len();
        if remaining.first() != Some(&b':') {
            return None;
        }
        cursor += 1;

        let remaining = skip(&object[cursor..]);
        cursor = object.len() - remaining.len();
        let end = json_value_end(remaining)?;
        let field_value = &remaining[..end];
        cursor += end;
        if key_token == key {
            if found.is_some() {
                return None;
            }
            found = Some(field_value);
        }

        let remaining = skip(&object[cursor..]);
        cursor = object.len() - remaining.len();
        match remaining.first()? {
            b',' => cursor += 1,
            b'}' => {}
            _ => return None,
        }
    }
}

fn json_value_end(value: &[u8]) -> Option<usize> {
    match value.first()? {
        b'{' | b'[' => balanced_end(value).map(|end| end + 1),
        b'\"' => string_end(value).map(|end| end + 1),
        _ => value
            .iter()
            .position(|byte| matches!(*byte, b',' | b'}' | b']' | b' ' | b'\n' | b'\r' | b'\t'))
            .or(Some(value.len())),
    }
}

fn balanced_end(value: &[u8]) -> Option<usize> {
    let mut depth = 0i32;
    let mut quoted = false;
    let mut escaped = false;
    for (index, byte) in value.iter().enumerate() {
        if quoted {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'\"' {
                quoted = false;
            }
            continue;
        }
        match *byte {
            b'\"' => quoted = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn string_end(value: &[u8]) -> Option<usize> {
    let mut escaped = false;
    for (index, byte) in value.iter().enumerate().skip(1) {
        if escaped {
            escaped = false;
        } else if *byte == b'\\' {
            escaped = true;
        } else if *byte == b'\"' {
            return Some(index);
        }
    }
    None
}

fn string_after<'a>(value: &'a [u8], key: &[u8]) -> &'a [u8] {
    let Some(encoded) = value_after(value, key) else {
        return b"";
    };
    if encoded.len() < 2 || encoded[0] != b'\"' || encoded[encoded.len() - 1] != b'\"' {
        return b"";
    }
    &encoded[1..encoded.len() - 1]
}

fn int_value_after(value: &[u8], key: &[u8]) -> Option<i32> {
    let encoded = value_after(value, key)?;
    let mut result = 0i32;
    if encoded.is_empty() {
        return None;
    }
    for byte in encoded {
        if !byte.is_ascii_digit() {
            return None;
        }
        result = result.checked_mul(10)?.checked_add((byte - b'0') as i32)?;
    }
    Some(result)
}

fn copy(output: &mut [u8], at: usize, value: &[u8]) -> usize {
    let Some(end) = at.checked_add(value.len()) else {
        return 0;
    };
    if end > output.len() {
        return 0;
    }
    output[at..end].copy_from_slice(value);
    end
}

fn append(output: &mut [u8], at: &mut usize, value: &[u8]) -> bool {
    let next = copy(output, *at, value);
    if next == 0 {
        return false;
    }
    *at = next;
    true
}

fn append_json_string(output: &mut [u8], at: &mut usize, value: &[u8]) -> bool {
    for byte in value {
        if !append(output, at, &[*byte]) {
            return false;
        }
    }
    true
}

fn append_i32(output: &mut [u8], at: &mut usize, mut value: i32) -> bool {
    if value == 0 {
        return append(output, at, b"0");
    }
    let mut digits = [0u8; 10];
    let mut count = 0usize;
    while value > 0 {
        digits[count] = b'0' + (value % 10) as u8;
        value /= 10;
        count += 1;
    }
    while count > 0 {
        count -= 1;
        if !append(output, at, &[digits[count]]) {
            return false;
        }
    }
    true
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
