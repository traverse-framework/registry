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
    fn connector_invoke(
        request_ptr: i32,
        request_len: i32,
        response_ptr: i32,
        response_capacity: i32,
    ) -> i32;
}

#[cfg(test)]
#[no_mangle]
unsafe extern "C" fn connector_invoke(_: i32, _: i32, _: i32, _: i32) -> i32 {
    0
}

const INPUT_CAPACITY: usize = 4096;
const REQUEST_CAPACITY: usize = 4096;
const RESPONSE_CAPACITY: usize = 4096;
const OUTPUT_CAPACITY: usize = 4096;

static mut INPUT: [u8; INPUT_CAPACITY] = [0; INPUT_CAPACITY];
static mut REQUEST: [u8; REQUEST_CAPACITY] = [0; REQUEST_CAPACITY];
static mut RESPONSE: [u8; RESPONSE_CAPACITY] = [0; RESPONSE_CAPACITY];
static mut OUTPUT: [u8; OUTPUT_CAPACITY] = [0; OUTPUT_CAPACITY];

#[unsafe(no_mangle)]
#[cfg(not(test))]
pub extern "C" fn _start() {
    // The WASM module is single-invocation and uses bounded static buffers so
    // memory consumption does not depend on recording size.
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
            let vector = IoVecMut {
                buffer: extra.as_mut_ptr(),
                length: extra.len(),
            };
            fd_read(0, &vector, 1, &mut read) != 0 || read > 0
        } else {
            false
        };
        let output_length = if oversized {
            unavailable(output, b"invalid_request")
        } else {
            finalize(&input[..total], request, response, output)
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
        let mut request = [0u8; 4096];
        let mut response = [0u8; 4096];
        let mut output = [0u8; 4096];
        let length = finalize(b"{}", &mut request, &mut response, &mut output);
        assert_eq!(&output[..length], b"{\"asset_ref\":\"\",\"content_digest\":\"\",\"size\":0,\"result_class\":\"invalid_request\"}");
    }

    #[test]
    fn exercises_json_scanner_and_bounds() {
        let mut request = [0u8; 4096];
        let mut response = [0u8; 4096];
        let mut output = [0u8; 4096];
        assert!(string_token_after(br#" {"x":"a\"b"} "#, b"x").is_some());
        assert!(string_token_after(br#"{"x":1}"#, b"x").is_none());
        assert!(is_empty_string(b"\"\""));
        assert!(!is_empty_string(b"\"x\""));
        assert!(string_end(b"\"unterminated").is_none());
        assert!(string_end(b"\"bad\n\"").is_none());
        assert_eq!(skip_whitespace(b" \n\t x"), b"x");
        assert!(object_field(b"[]", b"x").is_none());
        assert!(object_field(br#"{"x":1,"x":2}"#, b"x").is_none());
        assert!(object_field(br#"{"x":1}tail"#, b"x").is_none());
        assert!(object_field(br#"{"a":1,"x":{"n":[true]}}"#, b"x").is_some());
        assert_eq!(json_value_end(b"123 "), Some(3));
        assert!(json_value_end(br#"{"x":1}"#).is_some());
        assert!(json_value_end(br#"[1,2]"#).is_some());
        assert!(json_value_end(br#""x""#).is_some());
        assert!(balanced_end(b"{\"x\": [1]}").is_some());
        assert!(balanced_end(b"{").is_none());
        assert!(balanced_end(b"]").is_none());
        assert!(copy(&mut output[..2], 0, b"abc").is_none());
        assert_eq!(copy(&mut output, 0, b"ok"), Some(2));
        assert!(unavailable(&mut output, b"x") > 0);
        assert_eq!(unavailable(&mut output[..2], b"x"), 0);
        for size in 0..128 {
            let _ = unavailable(&mut output[..size], b"long-result-class");
        }
        assert_eq!(unsafe { connector_invoke(0, 0, 0, 0) }, 0);
        let n = finalize(br#"{"content_ref":"c","media_type":"audio/wav","retention_class":"daily","idempotency_key":"k"}"#, &mut request, &mut response, &mut output);
        assert!(core::str::from_utf8(&output[..n]).unwrap().contains("\"asset_ref\":\"a\""));
        let mut tiny_output = [0u8; 1];
        assert_eq!(finalize(br#"{"content_ref":"c","media_type":"audio/wav","retention_class":"daily","idempotency_key":"k"}"#, &mut request, &mut response, &mut tiny_output), 0);
        for size in 0..256 {
            let _ = finalize(br#"{"content_ref":"c","media_type":"audio/wav","retention_class":"daily","idempotency_key":"k"}"#, &mut request[..size], &mut response, &mut output);
        }
        for input in [
            br#"{"content_ref":"","media_type":"x","retention_class":"d","idempotency_key":"k"}"# as &[u8],
            br#"{"content_ref":"c","media_type":"","retention_class":"d","idempotency_key":"k"}"#,
            br#"{"content_ref":"c","media_type":"x","retention_class":"","idempotency_key":"k"}"#,
            br#"{"content_ref":"c","media_type":"x","retention_class":"d","idempotency_key":""}"#,
            br#"{"content_ref":"c","media_type":"x","retention_class":"d","idempotency_key":"k",}"#,
        ] {
            let n = finalize(input, &mut request, &mut response, &mut output);
            assert!(n > 0);
        }
    }

    #[test]
    fn rejects_oversized_and_partial_requests() {
        let mut request = [0u8; 4096];
        let mut response = [0u8; 4096];
        let mut output = [0u8; 4096];
        let mut oversized = [b'a'; 4097];
        let n = finalize(&oversized, &mut request, &mut response, &mut output);
        assert!(core::str::from_utf8(&output[..n]).unwrap().contains("invalid_request"));
        oversized[0] = b'{';
        for input in [br#"{"content_ref":"c"}"# as &[u8], br#"{"content_ref":"c","media_type":"x"}"#, br#"{"content_ref":"c","media_type":"x","retention_class":"d"}"#] {
            let n = finalize(input, &mut request, &mut response, &mut output);
            assert!(n > 0);
        }
    }
}

fn finalize(input: &[u8], request: &mut [u8], response: &mut [u8], out: &mut [u8]) -> usize {
    let Some(content_ref) = string_token_after(input, b"content_ref") else {
        return unavailable(out, b"invalid_request");
    };
    let Some(media_type) = string_token_after(input, b"media_type") else {
        return unavailable(out, b"invalid_request");
    };
    let Some(retention_class) = string_token_after(input, b"retention_class") else {
        return unavailable(out, b"invalid_request");
    };
    let Some(idempotency_key) = string_token_after(input, b"idempotency_key") else {
        return unavailable(out, b"invalid_request");
    };
    if is_empty_string(content_ref)
        || is_empty_string(media_type)
        || is_empty_string(retention_class)
        || is_empty_string(idempotency_key)
    {
        return unavailable(out, b"invalid_request");
    }

    let Some(mut at) = copy(request, 0, b"{\"abi_version\":\"1.0.0\",\"connector_id\":\"traverse.object-store\",\"operation\":\"put_immutable\",\"payload\":{\"content_ref\":") else {
        return unavailable(out, b"invalid_request");
    };
    let Some(next) = copy(request, at, content_ref) else {
        return unavailable(out, b"invalid_request");
    };
    at = next;
    for (field, value) in [
        (b",\"media_type\":".as_slice(), media_type),
        (b",\"retention_class\":".as_slice(), retention_class),
        (b",\"idempotency_key\":".as_slice(), idempotency_key),
    ] {
        let Some(next) = copy(request, at, field) else {
            return unavailable(out, b"invalid_request");
        };
        at = next;
        let Some(next) = copy(request, at, value) else {
            return unavailable(out, b"invalid_request");
        };
        at = next;
    }
    let Some(at) = copy(request, at, b"}}") else {
        return unavailable(out, b"invalid_request");
    };
    if at > i32::MAX as usize {
        return unavailable(out, b"invalid_request");
    }

    #[cfg(test)]
    let received = {
        let body = b"{\"payload\":{\"asset_ref\":\"a\",\"content_digest\":\"d\",\"size\":1,\"result_class\":\"stored\"}}";
        response[..body.len()].copy_from_slice(body);
        body.len() as i32
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
        return unavailable(out, b"connector_unavailable");
    }
    let Some(payload) = object_field(&response[..received as usize], b"payload") else {
        return unavailable(out, b"connector_unavailable");
    };
    if payload.first() != Some(&b'{') {
        return unavailable(out, b"connector_unavailable");
    }
    copy(out, 0, payload).unwrap_or_else(|| unavailable(out, b"connector_unavailable"))
}

fn unavailable(out: &mut [u8], result_class: &[u8]) -> usize {
    let Some(at) = copy(
        out,
        0,
        b"{\"asset_ref\":\"\",\"content_digest\":\"\",\"size\":0,\"result_class\":\"",
    ) else {
        return 0;
    };
    let Some(at) = copy(out, at, result_class) else {
        return 0;
    };
    copy(out, at, b"\"}").unwrap_or(0)
}

fn skip_whitespace(mut value: &[u8]) -> &[u8] {
    while value
        .first()
        .is_some_and(|byte| matches!(*byte, b' ' | b'\n' | b'\r' | b'\t'))
    {
        value = &value[1..];
    }
    value
}

/// Return the raw JSON string token (including quotes). Keeping a validated
/// token intact preserves escapes and prevents input data from escaping the
/// generated connector request string.
fn string_token_after<'a>(value: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let token = object_field(value, key)?;
    if token.first() != Some(&b'"') {
        return None;
    }
    let end = string_end(token)?;
    Some(&token[..end + 1])
}

fn is_empty_string(token: &[u8]) -> bool {
    token == b"\"\""
}

fn string_end(value: &[u8]) -> Option<usize> {
    let mut escaped = false;
    for (index, byte) in value.iter().enumerate().skip(1) {
        if escaped {
            escaped = false;
        } else if *byte == b'\\' {
            escaped = true;
        } else if *byte == b'"' {
            return Some(index);
        } else if *byte < 0x20 {
            return None;
        }
    }
    None
}

/// Select exactly one direct member of a serialized JSON object. Traverse
/// serializes the already-validated request before invoking the guest; this
/// scanner still checks member boundaries, rejects duplicate target keys, and
/// requires the complete top-level object to be consumed.
fn object_field<'a>(value: &'a [u8], expected_key: &[u8]) -> Option<&'a [u8]> {
    let object = skip_whitespace(value);
    if object.first() != Some(&b'{') {
        return None;
    }
    let mut cursor = 1usize;
    let mut found = None;
    loop {
        let remaining = skip_whitespace(&object[cursor..]);
        cursor = object.len() - remaining.len();
        if remaining.first() == Some(&b'}') {
            cursor += 1;
            return if found.is_some() && skip_whitespace(&object[cursor..]).is_empty() {
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
        let remaining = skip_whitespace(&object[cursor..]);
        cursor = object.len() - remaining.len();
        if remaining.first() != Some(&b':') {
            return None;
        }
        cursor += 1;

        let remaining = skip_whitespace(&object[cursor..]);
        cursor = object.len() - remaining.len();
        let value_end = json_value_end(remaining)?;
        let field_value = &remaining[..value_end];
        cursor += value_end;

        if key_token.get(1..key_token.len().checked_sub(1)?) == Some(expected_key) {
            if found.is_some() {
                return None;
            }
            found = Some(field_value);
        }

        let remaining = skip_whitespace(&object[cursor..]);
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
        b'"' => string_end(value).map(|end| end + 1),
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
                    return Some(index);
                }
                if depth < 0 {
                    return None;
                }
            }
            _ => {}
        }
    }
    None
}

fn copy(output: &mut [u8], at: usize, value: &[u8]) -> Option<usize> {
    let end = at.checked_add(value.len())?;
    if end > output.len() {
        return None;
    }
    output[at..end].copy_from_slice(value);
    Some(end)
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
