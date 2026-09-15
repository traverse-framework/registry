#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#[cfg(test)]
extern crate std;

const MAX_INPUT: usize = 4096;
const MAX_OUTPUT: usize = 1024;
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
    let asset_ref = string_after(input, b"\"asset_ref\"");
    let request_id = string_after(input, b"\"request_id\"");
    let state = string_after(input, b"\"retention_state\"");
    let Some(now) = number_after(input, b"\"evaluation_time_epoch_seconds\"") else {
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
            b"\"request_id\"",
            b"\"retention_state\"",
            b"\"evaluation_time_epoch_seconds\"",
            b"\"policy\"",
        ],
    ) || !unique_fields(
        policy,
        &[
            b"\"version\"",
            b"\"eligible_action\"",
            b"\"grace_period_seconds\"",
            b"\"approval_required\"",
        ],
    ) {
        return error(output, b"invalid_request");
    }
    let version = string_after(policy, b"\"version\"");
    let action = string_after(policy, b"\"eligible_action\"");
    let Some(grace) = number_after(policy, b"\"grace_period_seconds\"") else {
        return error(output, b"invalid_request");
    };
    let Some(approval_required) = boolean_after(policy, b"\"approval_required\"") else {
        return error(output, b"invalid_request");
    };
    if !safe_token(asset_ref, 256)
        || !safe_token(request_id, 128)
        || !safe_token(version, 128)
        || !matches!(state, b"held" | b"retained" | b"eligible")
        || !matches!(action, b"archive" | b"delete")
        || grace > 31_536_000
    {
        return error(output, b"invalid_request");
    }
    let mut w = Writer {
        bytes: output,
        at: 0,
    };
    w.bytes(b"{\"asset_ref\":\"");
    w.bytes(asset_ref);
    w.bytes(b"\",\"request_id\":\"");
    w.bytes(request_id);
    if state != b"eligible" {
        w.bytes(b"\",\"plan_state\":\"no_action\",\"action\":\"none\",\"reason\":\"not_eligible\",\"policy_version\":\"");
        w.bytes(version);
        w.bytes(b"\"}");
        return w.at;
    }
    let Some(execute_after) = now.checked_add(grace) else {
        return error(output, b"time_overflow");
    };
    w.bytes(b"\",\"plan_state\":\"planned\",\"action\":\"");
    w.bytes(action);
    w.bytes(b"\",\"approval_state\":\"");
    w.bytes(if approval_required {
        b"required"
    } else {
        b"not_required"
    });
    w.bytes(b"\",\"execute_after_epoch_seconds\":");
    w.number(execute_after);
    w.bytes(b",\"reason\":\"eligible_after_grace\",\"policy_version\":\"");
    w.bytes(version);
    w.bytes(b"\"}");
    w.at
}

fn unique_fields(input: &[u8], keys: &[&[u8]]) -> bool {
    keys.iter().all(|key| {
        let key = &key[1..key.len() - 1];
        let mut count = 0usize;
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
        && value.iter().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(*b, b'.' | b'_' | b':' | b'/' | b'@' | b'+' | b'-')
        })
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
fn number_after(input: &[u8], key: &[u8]) -> Option<u64> {
    let value = value_after(input, key)?;
    if value.is_empty() || !value.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut result = 0u64;
    for byte in value {
        result = result.checked_mul(10)?.checked_add((byte - b'0') as u64)?;
    }
    Some(result)
}
fn value_after<'a>(input: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let position = input.windows(key.len()).position(|window| window == key)?;
    let colon = input[position + key.len()..]
        .iter()
        .position(|b| *b == b':')?
        + position
        + key.len()
        + 1;
    let mut start = colon;
    spaces(input, &mut start);
    let first = *input.get(start)?;
    let end = if first == b'{' {
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
                b'{' => depth += 1,
                b'}' => {
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
fn spaces(input: &[u8], at: &mut usize) {
    while input
        .get(*at)
        .is_some_and(|b| matches!(*b, b' ' | b'\n' | b'\r' | b'\t'))
    {
        *at += 1;
    }
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
    fn number(&mut self, value: u64) {
        let mut buffer = [0u8; 20];
        let mut number = value;
        let mut index = buffer.len();
        loop {
            index -= 1;
            buffer[index] = b'0' + (number % 10) as u8;
            number /= 10;
            if number == 0 {
                break;
            }
        }
        self.bytes(&buffer[index..]);
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
#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}

#[cfg(test)]
mod tests {
    use super::{boolean_after, number_after, object_after, plan, spaces, string_after, unique_fields, value_after, Writer};

    const ELIGIBLE: &str = r#"{"asset_ref":"artifact:42","request_id":"retention-plan-42","retention_state":"eligible","evaluation_time_epoch_seconds":1000,"policy":{"version":"retain-1","eligible_action":"archive","grace_period_seconds":86400,"approval_required":true}}"#;

    fn run(input: &str) -> std::string::String {
        let mut output = [0u8; 1024];
        let length = plan(input.as_bytes(), &mut output);
        std::string::String::from_utf8(output[..length].to_vec()).unwrap()
    }

    #[test]
    fn creates_delayed_approval_gated_plan() {
        let output = run(ELIGIBLE);
        assert!(output.contains("\"execute_after_epoch_seconds\":87400"));
        assert!(output.contains("\"approval_state\":\"required\""));
    }

    #[test]
    fn held_and_retained_states_produce_no_action() {
        for state in ["held", "retained"] {
            let input = ELIGIBLE.replace("\"retention_state\":\"eligible\"", &format!("\"retention_state\":\"{state}\""));
            assert!(run(&input).contains("\"plan_state\":\"no_action\""));
        }
    }

    #[test]
    fn applies_delete_without_approval_when_configured() {
        let input = ELIGIBLE
            .replace("\"eligible_action\":\"archive\"", "\"eligible_action\":\"delete\"")
            .replace("\"approval_required\":true", "\"approval_required\":false");
        let output = run(&input);
        assert!(output.contains("\"action\":\"delete\""));
        assert!(output.contains("\"approval_state\":\"not_required\""));
    }

    #[test]
    fn rejects_overflow_invalid_fields_and_unbounded_grace_periods() {
        for input in [
            ELIGIBLE.replace("1000", "18446744073709551615"),
            ELIGIBLE.replace("\"approval_required\":true", "\"approval_required\":\"yes\""),
            ELIGIBLE.replace("\"grace_period_seconds\":86400", "\"grace_period_seconds\":31536001"),
            ELIGIBLE.replace("artifact:42", "../unsafe"),
            ELIGIBLE.replace("\"retention_state\":\"eligible\"", "\"retention_state\":\"unknown\""),
            "{}".to_string(),
        ] {
            assert!(run(&input).contains("result_class"));
        }
        assert!(run(&ELIGIBLE.replace("86400", "1").replace("1000", "18446744073709551615")).contains("time_overflow"));
        assert!(run(&std::string::String::from_utf8(vec![b'x'; 4097]).unwrap()).contains("input_limit_exceeded"));
    }

    #[test]
    fn rejects_duplicate_fields_missing_grace_and_missing_policy() {
        let duplicate = ELIGIBLE.replace("\"asset_ref\":", "\"asset_ref\":\"artifact:other\",\"asset_ref\":");
        assert!(run(&duplicate).contains("invalid_request"));
        let missing_grace = ELIGIBLE.replace(",\"grace_period_seconds\":86400", "");
        assert!(run(&missing_grace).contains("invalid_request"));
        let invalid_policy = ELIGIBLE.replace("\"policy\":{", "\"policy\":[]");
        assert!(run(&invalid_policy).contains("invalid_request"));
    }

    #[test]
    fn json_helpers_handle_malformed_and_escaped_boundaries() {
        assert_eq!(object_after(b"{\"x\":[]}", b"\"x\""), None);
        assert_eq!(string_after(b"{\"x\":true}", b"\"x\""), b"");
        assert_eq!(boolean_after(b"{\"x\":0}", b"\"x\""), None);
        assert_eq!(number_after(b"{\"x\":18446744073709551616}", b"\"x\""), None);
        assert_eq!(value_after(b"{\"x\":\"unterminated", b"\"x\""), None);
        assert_eq!(value_after(b"{\"x\":[1,2", b"\"x\""), Some(&b"[1"[..]));
        let escaped = br#"{"x":{"text":"a\\\"b"},"y":0}"#;
        assert!(value_after(escaped, b"\"x\"").is_some());
        let mut offset = 0;
        spaces(b" \n\r\tX", &mut offset);
        assert_eq!(offset, 4);
        assert!(!unique_fields(b"{\"x\":1,\"x\":2}", &[b"\"x\""]));
    }

    #[test]
    fn writer_bounds_are_checked() {
        let mut output = [0u8; 1];
        let mut writer = Writer { bytes: &mut output, at: 0 };
        writer.bytes(b"too long");
        assert_eq!(writer.at, 0);
    }
}
