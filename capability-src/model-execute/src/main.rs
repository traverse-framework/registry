// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 Traverse contributors

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
use std::sync::atomic::{AtomicU8, Ordering};
#[cfg(test)]
use std::sync::Mutex;

#[cfg(test)]
static TEST_CONNECTOR_MODE: AtomicU8 = AtomicU8::new(0);

#[cfg(test)]
static TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
static mut TEST_CONNECTOR_BODY: [u8; 4096] = [0; 4096];

#[cfg(test)]
static mut TEST_CONNECTOR_BODY_LEN: usize = 0;

#[cfg(test)]
#[no_mangle]
unsafe extern "C" fn connector_invoke(_: i32, _: i32, _: i32, _: i32) -> i32 {
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
        let response =
            core::slice::from_raw_parts_mut(response_ptr.cast::<u8>(), RESPONSE_CAPACITY);
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
            fail_closed(output, b"invalid_request")
        } else {
            execute_model(&input[..total], request, response, output)
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

    fn set_body(body: &[u8]) {
        unsafe {
            TEST_CONNECTOR_BODY[..body.len()].copy_from_slice(body);
            TEST_CONNECTOR_BODY_LEN = body.len();
        }
        TEST_CONNECTOR_MODE.store(0, Ordering::Relaxed);
    }

    #[test]
    fn rejects_missing_required_fields() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut request = [0u8; 8192];
        let mut response = [0u8; 4096];
        let mut output = [0u8; 4096];
        let n = execute_model(b"{}", &mut request, &mut response, &mut output);
        assert!(core::str::from_utf8(&output[..n])
            .unwrap()
            .contains("invalid_request"));
    }

    #[test]
    fn rejects_undeclared_usage() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut request = [0u8; 8192];
        let mut response = [0u8; 4096];
        let mut output = [0u8; 4096];
        let input = br#"{"model_ref":{"model_id":"traverse.digits-onnx","version":"1.0.0","digest":"sha256:ecc32b909bd495c975e2c07a0f73ed85929026325bd8cf7865b9eec9293e03cb"},"input_ref":"in:1","policy_ref":"policy:default","data_classification":"public","input_schema_ref":"schema:in","input_schema_version":"1.0.0","max_output_bytes":56,"trust_root":{"allowed_signers":["test-key"]}}"#;
        let n = execute_model(input, &mut request, &mut response, &mut output);
        assert!(core::str::from_utf8(&output[..n])
            .unwrap()
            .contains("usage_undeclared"));
    }

    #[test]
    fn rejects_empty_trust_root() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut request = [0u8; 8192];
        let mut response = [0u8; 4096];
        let mut output = [0u8; 4096];
        let input = br#"{"model_ref":{"model_id":"traverse.digits-onnx","version":"1.0.0","digest":"sha256:ecc32b909bd495c975e2c07a0f73ed85929026325bd8cf7865b9eec9293e03cb"},"input_ref":"in:1","policy_ref":"policy:default","data_classification":"public","input_schema_ref":"schema:in","input_schema_version":"1.0.0","max_output_bytes":56,"model_usage":"non_commercial","trust_root":{"allowed_signers":[]}}"#;
        let n = execute_model(input, &mut request, &mut response, &mut output);
        assert!(core::str::from_utf8(&output[..n])
            .unwrap()
            .contains("key_untrusted"));
    }

    #[test]
    fn happy_path_surfaces_model_evidence() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut request = [0u8; 8192];
        let mut response = [0u8; 4096];
        let mut output = [0u8; 4096];
        // Normative flat model_evidence (registry#641): license_id, commercial_use,
        // status, effective_usage — no nested rights.source_url (Spec 104 rejects ://).
        set_body(br#"{"abi_version":"1.0.0","result_class":"ok","payload":{"status":"ok","output_ref":"out:1","output_schema_ref":"schema:out","model_ref":{"model_id":"traverse.digits-onnx","version":"1.0.0","digest":"sha256:ecc32b909bd495c975e2c07a0f73ed85929026325bd8cf7865b9eec9293e03cb"},"placement":"wasm-cpu","result_class":"ok","model_evidence":{"license_id":"CC-BY-4.0","commercial_use":"allowed","status":"active","effective_usage":"non_commercial","derivation":{"kind":"quantized-onnx"}}}}"#);
        let input = br#"{"model_ref":{"model_id":"traverse.digits-onnx","version":"1.0.0","digest":"sha256:ecc32b909bd495c975e2c07a0f73ed85929026325bd8cf7865b9eec9293e03cb"},"input_ref":"in:1","policy_ref":"policy:default","data_classification":"public","input_schema_ref":"schema:in","input_schema_version":"1.0.0","max_output_bytes":56,"model_usage":"non_commercial","trust_root":{"allowed_signers":["test-key"]}}"#;
        let n = execute_model(input, &mut request, &mut response, &mut output);
        let text = core::str::from_utf8(&output[..n]).unwrap();
        assert!(text.contains("\"status\":\"ok\""));
        assert!(text.contains("model_evidence"));
        assert!(text.contains("CC-BY-4.0"));
        assert!(text.contains("\"effective_usage\":\"non_commercial\""));
        assert!(text.contains("\"status\":\"active\""));
        assert!(!text.contains("source_url"));
        assert!(!text.contains("https://"));
        assert!(core::str::from_utf8(&request[..])
            .unwrap()
            .contains("traverse.model-runtime"));
        assert!(core::str::from_utf8(&request[..])
            .unwrap()
            .contains("model.execute"));
    }

    #[test]
    fn passes_through_fail_closed_reasons() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut request = [0u8; 8192];
        let mut response = [0u8; 4096];
        let mut output = [0u8; 4096];
        let input = br#"{"model_ref":{"model_id":"traverse.digits-onnx","version":"1.0.0","digest":"sha256:ecc32b909bd495c975e2c07a0f73ed85929026325bd8cf7865b9eec9293e03cb"},"input_ref":"in:1","policy_ref":"policy:default","data_classification":"public","input_schema_ref":"schema:in","input_schema_version":"1.0.0","max_output_bytes":56,"model_usage":"commercial","trust_root":{"allowed_signers":["test-key"]}}"#;
        for reason in [
            "digest_mismatch",
            "key_untrusted",
            "signature_invalid",
            "rights_mismatch",
            "rights_policy_denied",
            "usage_undeclared",
            "package_revoked",
            "model_unavailable",
        ]
        .iter()
        {
            let mut body = [0u8; 256];
            let mut at = 0usize;
            at = copy(&mut body, at, br#"{"abi_version":"1.0.0","result_class":"model_unavailable","payload":{"status":"error","reason":""#);
            at = copy(&mut body, at, reason.as_bytes());
            at = copy(&mut body, at, br#""}}"#);
            set_body(&body[..at]);
            let n = execute_model(input, &mut request, &mut response, &mut output);
            let text = core::str::from_utf8(&output[..n]).unwrap();
            assert!(text.contains(reason), "{}", text);
            assert!(text.contains("\"status\":\"error\""));
        }
    }

    #[test]
    fn unbound_connector_fails_closed() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut request = [0u8; 8192];
        let mut response = [0u8; 4096];
        let mut output = [0u8; 4096];
        TEST_CONNECTOR_MODE.store(2, Ordering::Relaxed);
        let input = br#"{"model_ref":{"model_id":"traverse.digits-onnx","version":"1.0.0","digest":"sha256:ecc32b909bd495c975e2c07a0f73ed85929026325bd8cf7865b9eec9293e03cb"},"input_ref":"in:1","policy_ref":"policy:default","data_classification":"public","input_schema_ref":"schema:in","input_schema_version":"1.0.0","max_output_bytes":56,"model_usage":"non_commercial","trust_root":{"allowed_signers":["test-key"]}}"#;
        let n = execute_model(input, &mut request, &mut response, &mut output);
        assert!(core::str::from_utf8(&output[..n])
            .unwrap()
            .contains("connector_unavailable"));
        TEST_CONNECTOR_MODE.store(0, Ordering::Relaxed);
    }

    #[test]
    fn exercises_helpers() {
        assert_eq!(unsafe { connector_invoke(0, 0, 0, 0) }, 0);
        assert_eq!(skip(b" \n\tx"), b"x");
        assert!(value_after(b"[]", b"x").is_none());
        assert!(value_after(br#"{"a":1,"a":2}"#, b"\"a\"").is_none());
        assert!(json_value_end(br#"{"x":1}"#).is_some());
        assert!(json_value_end(br#"[1]"#).is_some());
        assert!(json_value_end(br#""x""#).is_some());
        assert!(balanced_end(br#"{"x":"escaped\\\"quote"}"#).is_some());
        assert!(balanced_end(b"{").is_none());
        assert!(string_end(b"\"x\"").is_some());
        assert!(string_end(b"\"x").is_none());
        assert_eq!(string_after(br#"{"k":"value"}"#, b"\"k\""), b"value");
        assert_eq!(string_after(br#"{"k":1}"#, b"\"k\""), b"");
        assert_eq!(int_value_after(br#"{"n":12}"#, b"\"n\""), Some(12));
        assert_eq!(int_value_after(br#"{"n":0}"#, b"\"n\""), Some(0));
        assert!(int_value_after(br#"{"n":"x"}"#, b"\"n\"").is_none());
        let mut tiny = [0u8; 1];
        let mut at = 1;
        assert!(!append(&mut tiny, &mut at, b"x"));
        assert!(!append_json_string(&mut tiny, &mut at, b"x"));
        assert!(!append_i32(&mut tiny, &mut at, 7));
        assert!(append_i32(
            &mut [0u8; 8],
            &mut {
                let mut a = 0;
                a
            },
            0
        ));
        assert_eq!(fail_closed(&mut tiny, b"x"), 0);
        assert!(array_nonempty(b"[\"a\"]"));
        assert!(!array_nonempty(b"[]"));
        assert!(!array_nonempty(b"{}"));
        assert!(!array_nonempty(b"x"));
    }

    #[test]
    fn invalid_model_ref_and_empty_connector_payload() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut request = [0u8; 8192];
        let mut response = [0u8; 4096];
        let mut output = [0u8; 4096];
        for invalid in [
            br#"{"model_ref":"x"}"# as &[u8],
            br#"{"model_ref":{"model_id":"","version":"1","digest":"d"},"input_ref":"i","policy_ref":"p","data_classification":"public","input_schema_ref":"s","input_schema_version":"1","max_output_bytes":1,"model_usage":"commercial","trust_root":{"allowed_signers":["k"]}}"#,
            br#"{"model_ref":{"model_id":"m","version":"1","digest":"d"},"input_ref":"","policy_ref":"p","data_classification":"public","input_schema_ref":"s","input_schema_version":"1","max_output_bytes":1,"model_usage":"commercial","trust_root":{"allowed_signers":["k"]}}"#,
            br#"{"model_ref":{"model_id":"m","version":"1","digest":"d"},"input_ref":"i","policy_ref":"p","data_classification":"public","input_schema_ref":"s","input_schema_version":"1","max_output_bytes":1,"model_usage":"other","trust_root":{"allowed_signers":["k"]}}"#,
            br#"{"model_ref":{"model_id":"m","version":"1","digest":"d"},"input_ref":"i","policy_ref":"p","data_classification":"public","input_schema_ref":"s","input_schema_version":"1","max_output_bytes":1,"model_usage":"commercial"}"#,
            br#"{"model_ref":{"model_id":"m","version":"1","digest":"d"},"input_ref":"i","policy_ref":"p","data_classification":"public","input_schema_ref":"s","input_schema_version":"1","max_output_bytes":1,"model_usage":"commercial","trust_root":{}}"#,
        ] {
            let n = execute_model(invalid, &mut request, &mut response, &mut output);
            assert!(n > 0);
            assert!(core::str::from_utf8(&output[..n]).unwrap().contains("\"status\":\"error\""));
        }
        TEST_CONNECTOR_MODE.store(1, Ordering::Relaxed);
        let input = br#"{"model_ref":{"model_id":"m","version":"1","digest":"d"},"input_ref":"i","policy_ref":"p","data_classification":"public","input_schema_ref":"s","input_schema_version":"1","max_output_bytes":1,"model_usage":"commercial","trust_root":{"allowed_signers":["k"]}}"#;
        let n = execute_model(input, &mut request, &mut response, &mut output);
        assert!(core::str::from_utf8(&output[..n])
            .unwrap()
            .contains("connector_unavailable"));
        TEST_CONNECTOR_MODE.store(0, Ordering::Relaxed);
        set_body(br#"{"abi_version":"1.0.0","result_class":"ok","payload":[]}"#);
        let n = execute_model(input, &mut request, &mut response, &mut output);
        assert!(core::str::from_utf8(&output[..n])
            .unwrap()
            .contains("connector_unavailable"));
        set_body(br#"{"abi_version":"1.0.0","reason":"digest_mismatch"}"#);
        let n = execute_model(input, &mut request, &mut response, &mut output);
        assert!(core::str::from_utf8(&output[..n])
            .unwrap()
            .contains("digest_mismatch"));
        let mut tiny_request = [0u8; 8];
        let n = execute_model(input, &mut tiny_request, &mut response, &mut output);
        assert!(n > 0);
    }
}

fn execute_model(
    input: &[u8],
    request: &mut [u8],
    response: &mut [u8],
    output: &mut [u8],
) -> usize {
    let Some(model_ref) = value_after(input, b"\"model_ref\"") else {
        return fail_closed(output, b"invalid_request");
    };
    if model_ref.first() != Some(&b'{') {
        return fail_closed(output, b"invalid_request");
    }
    let model_id = string_after(model_ref, b"\"model_id\"");
    let version = string_after(model_ref, b"\"version\"");
    let digest = string_after(model_ref, b"\"digest\"");
    if model_id.is_empty() || version.is_empty() || digest.is_empty() {
        return fail_closed(output, b"invalid_request");
    }

    let input_ref = string_after(input, b"\"input_ref\"");
    let policy_ref = string_after(input, b"\"policy_ref\"");
    let data_classification = string_after(input, b"\"data_classification\"");
    let input_schema_ref = string_after(input, b"\"input_schema_ref\"");
    let input_schema_version = string_after(input, b"\"input_schema_version\"");
    let Some(max_output_bytes) = int_value_after(input, b"\"max_output_bytes\"") else {
        return fail_closed(output, b"invalid_request");
    };
    if input_ref.is_empty()
        || policy_ref.is_empty()
        || data_classification.is_empty()
        || input_schema_ref.is_empty()
        || input_schema_version.is_empty()
        || max_output_bytes < 1
    {
        return fail_closed(output, b"invalid_request");
    }

    let model_usage = string_after(input, b"\"model_usage\"");
    if model_usage.is_empty() {
        return fail_closed(output, b"usage_undeclared");
    }
    if model_usage != b"commercial" && model_usage != b"non_commercial" {
        return fail_closed(output, b"usage_undeclared");
    }

    let Some(trust_root) = value_after(input, b"\"trust_root\"") else {
        return fail_closed(output, b"key_untrusted");
    };
    let Some(signers) = value_after(trust_root, b"\"allowed_signers\"") else {
        return fail_closed(output, b"key_untrusted");
    };
    if !array_nonempty(signers) {
        return fail_closed(output, b"key_untrusted");
    }

    let mut at = copy(
        request,
        0,
        b"{\"abi_version\":\"1.0.0\",\"connector_id\":\"traverse.model-runtime\",\"operation\":\"model.execute\",\"payload\":{",
    );
    if at == 0
        || !append(request, &mut at, b"\"model_ref\":")
        || !append(request, &mut at, model_ref)
        || !append(request, &mut at, b",\"input_ref\":\"")
        || !append_json_string(request, &mut at, input_ref)
        || !append(request, &mut at, b"\",\"policy_ref\":\"")
        || !append_json_string(request, &mut at, policy_ref)
        || !append(request, &mut at, b"\",\"data_classification\":\"")
        || !append_json_string(request, &mut at, data_classification)
        || !append(request, &mut at, b"\",\"input_schema_ref\":\"")
        || !append_json_string(request, &mut at, input_schema_ref)
        || !append(request, &mut at, b"\",\"input_schema_version\":\"")
        || !append_json_string(request, &mut at, input_schema_version)
        || !append(request, &mut at, b"\",\"max_output_bytes\":")
        || !append_i32(request, &mut at, max_output_bytes)
        || !append(request, &mut at, b",\"model_usage\":\"")
        || !append_json_string(request, &mut at, model_usage)
        || !append(request, &mut at, b"\",\"trust_root\":")
        || !append(request, &mut at, trust_root)
        || !append(request, &mut at, b"}}")
    {
        return fail_closed(output, b"invalid_request");
    }
    if at == 0 || at > request.len() || at > i32::MAX as usize {
        return fail_closed(output, b"invalid_request");
    }

    #[cfg(test)]
    let received = {
        let mode = TEST_CONNECTOR_MODE.load(Ordering::Relaxed);
        if mode == 2 {
            0
        } else {
            let body = if mode == 1 {
                b"{}" as &[u8]
            } else {
                unsafe { &TEST_CONNECTOR_BODY[..TEST_CONNECTOR_BODY_LEN] }
            };
            let body = if body.is_empty() {
                br#"{"abi_version":"1.0.0","result_class":"ok","payload":{"status":"ok","output_ref":"out:1","output_schema_ref":"schema:out","model_ref":{"model_id":"traverse.digits-onnx","version":"1.0.0","digest":"sha256:ecc32b909bd495c975e2c07a0f73ed85929026325bd8cf7865b9eec9293e03cb"},"placement":"wasm-cpu","result_class":"ok","model_evidence":{"license_id":"CC-BY-4.0","commercial_use":"allowed","status":"active","effective_usage":"non_commercial","derivation":{"kind":"quantized-onnx"}}}}"#
            } else {
                body
            };
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
        return fail_closed(output, b"connector_unavailable");
    }

    let body = &response[..received as usize];
    let top_reason = string_after(body, b"\"reason\"");
    if !top_reason.is_empty() {
        return error_with_reason(output, top_reason);
    }
    let Some(payload) = value_after(body, b"\"payload\"") else {
        return fail_closed(output, b"connector_unavailable");
    };
    if payload.first() != Some(&b'{') {
        return fail_closed(output, b"connector_unavailable");
    }
    let payload_reason = string_after(payload, b"\"reason\"");
    if !payload_reason.is_empty() {
        return error_with_reason(output, payload_reason);
    }
    copy(output, 0, payload)
}

fn fail_closed(output: &mut [u8], reason: &[u8]) -> usize {
    error_with_reason(output, reason)
}

fn error_with_reason(output: &mut [u8], reason: &[u8]) -> usize {
    let mut at = 0usize;
    at = copy(output, at, b"{\"status\":\"error\",\"output_ref\":\"\",\"output_schema_ref\":\"\",\"model_ref\":{\"model_id\":\"\",\"version\":\"\",\"digest\":\"\"},\"placement\":\"\",\"result_class\":\"error\",\"reason\":\"");
    at = copy(output, at, reason);
    copy(output, at, b"\"}")
}

fn array_nonempty(value: &[u8]) -> bool {
    let value = skip(value);
    if value.first() != Some(&b'[') {
        return false;
    }
    let inner = skip(&value[1..]);
    inner.first() != Some(&b']')
}

fn skip(mut value: &[u8]) -> &[u8] {
    while value
        .first()
        .is_some_and(|byte| matches!(*byte, b' ' | b'\n' | b'\r' | b'\t'))
    {
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
