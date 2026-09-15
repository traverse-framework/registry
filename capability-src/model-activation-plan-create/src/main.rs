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
    let model_ref = string_after(input, b"\"model_ref\"");
    let target = string_after(input, b"\"target\"");
    let licence_ref = string_after(input, b"\"licence_ref\"");
    let digest = string_after(input, b"\"digest_evidence\"");
    let signature = string_after(input, b"\"signature_evidence\"");
    let licence = string_after(input, b"\"licence_evidence\"");
    let compatibility = string_after(input, b"\"runtime_compatibility\"");
    let Some(memory_required) = number_after(input, b"\"memory_required_bytes\"") else {
        return error(output, b"invalid_request");
    };
    let policy = match object_after(input, b"\"policy\"") {
        Some(value) => value,
        None => return error(output, b"invalid_request"),
    };
    if !unique_fields(
        input,
        &[
            b"\"model_ref\"",
            b"\"target\"",
            b"\"licence_ref\"",
            b"\"digest_evidence\"",
            b"\"signature_evidence\"",
            b"\"licence_evidence\"",
            b"\"runtime_compatibility\"",
            b"\"memory_required_bytes\"",
            b"\"policy\"",
        ],
    ) || !unique_fields(
        policy,
        &[
            b"\"version\"",
            b"\"allowed_targets\"",
            b"\"maximum_memory_bytes\"",
            b"\"unknown_evidence_action\"",
        ],
    ) {
        return error(output, b"invalid_request");
    }
    let policy_version = string_after(policy, b"\"version\"");
    let allowed_targets = string_array(policy, b"\"allowed_targets\"");
    let Some(max_memory) = number_after(policy, b"\"maximum_memory_bytes\"") else {
        return error(output, b"invalid_request");
    };
    let unknown_action = string_after(policy, b"\"unknown_evidence_action\"");
    if !safe_token(model_ref, 256)
        || !safe_token(target, 64)
        || !safe_token(licence_ref, 256)
        || !safe_token(policy_version, 128)
        || !evidence(digest)
        || !evidence(signature)
        || !matches!(licence, b"accepted" | b"rejected" | b"unknown")
        || !evidence(compatibility)
        || allowed_targets.invalid
        || allowed_targets.count == 0
        || !matches!(unknown_action, b"review" | b"reject")
    {
        return error(output, b"invalid_request");
    }

    let (decision, reason): (&[u8], &[u8]) = if !allowed_targets.contains(target) {
        (b"reject", b"target_not_allowed")
    } else if memory_required > max_memory {
        (b"reject", b"memory_budget_exceeded")
    } else if digest == b"failed" {
        (b"reject", b"digest_verification_failed")
    } else if signature == b"failed" {
        (b"reject", b"signature_verification_failed")
    } else if licence == b"rejected" {
        (b"reject", b"licence_not_accepted")
    } else if compatibility == b"failed" {
        (b"reject", b"runtime_incompatible")
    } else if digest == b"unknown"
        || signature == b"unknown"
        || licence == b"unknown"
        || compatibility == b"unknown"
    {
        if unknown_action == b"reject" {
            (b"reject", b"verification_evidence_unknown")
        } else {
            (b"review_required", b"verification_evidence_unknown")
        }
    } else {
        (b"activation_planned", b"policy_satisfied")
    };
    let mut w = Writer {
        bytes: output,
        at: 0,
    };
    w.bytes(b"{\"model_ref\":\"");
    w.bytes(model_ref);
    w.bytes(b"\",\"target\":\"");
    w.bytes(target);
    w.bytes(b"\",\"licence_ref\":\"");
    w.bytes(licence_ref);
    w.bytes(b"\",\"decision\":\"");
    w.bytes(decision);
    w.bytes(b"\",\"reason\":\"");
    w.bytes(reason);
    w.bytes(b"\",\"policy_version\":\"");
    w.bytes(policy_version);
    w.bytes(b"\"}");
    w.at
}

fn evidence(value: &[u8]) -> bool {
    matches!(value, b"verified" | b"failed" | b"unknown")
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
    let mut list = StringList {
        values: [b""; 16],
        count: 0,
        invalid: false,
    };
    let Some(array) = value_after(input, key) else {
        list.invalid = true;
        return list;
    };
    if array.first() != Some(&b'[') {
        list.invalid = true;
        return list;
    }
    let mut cursor = 1;
    loop {
        spaces(array, &mut cursor);
        if array.get(cursor) == Some(&b']') {
            break;
        }
        if array.get(cursor) != Some(&b'\"') || list.count == 16 {
            list.invalid = true;
            return list;
        }
        let start = cursor + 1;
        cursor += 1;
        while cursor < array.len() && array[cursor] != b'\"' {
            if array[cursor] == b'\\' {
                list.invalid = true;
                return list;
            }
            cursor += 1;
        }
        if cursor == start || cursor >= array.len() || !safe_token(&array[start..cursor], 64) {
            list.invalid = true;
            return list;
        }
        list.values[list.count] = &array[start..cursor];
        list.count += 1;
        cursor += 1;
        spaces(array, &mut cursor);
        if array.get(cursor) == Some(&b',') {
            cursor += 1;
            let mut next = cursor;
            spaces(array, &mut next);
            if array.get(next) == Some(&b']') {
                list.invalid = true;
                return list;
            }
        } else if array.get(cursor) != Some(&b']') {
            list.invalid = true;
            return list;
        }
    }
    list
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
    if value.len() < 2 || value[0] != b'\"' || value[value.len() - 1] != b'\"' {
        return b"";
    }
    &value[1..value.len() - 1]
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
                        found = Some(start + offset + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        found?
    } else if first == b'\"' {
        let mut offset = start + 1;
        let mut escaped = false;
        while offset < input.len() {
            if escaped {
                escaped = false;
            } else if input[offset] == b'\\' {
                escaped = true;
            } else if input[offset] == b'\"' {
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
fn safe_token(value: &[u8], maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value[0].is_ascii_alphanumeric()
        && value.iter().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(*b, b'.' | b'_' | b':' | b'/' | b'@' | b'+' | b'-')
        })
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
#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}

#[cfg(test)]
mod tests {
    use super::{evaluate, string_array, unique_fields, value_after};

    const ELIGIBLE: &str = r#"{"model_ref":"model:sha256:abc","target":"wasm32-wasip1","licence_ref":"licence:mit","digest_evidence":"verified","signature_evidence":"verified","licence_evidence":"accepted","runtime_compatibility":"verified","memory_required_bytes":1024,"policy":{"version":"1.0.0","allowed_targets":["wasm32-wasip1"],"maximum_memory_bytes":2048,"unknown_evidence_action":"review"}}"#;

    fn run(input: &str) -> std::string::String {
        let mut output = [0u8; 1024];
        let length = evaluate(input.as_bytes(), &mut output);
        std::string::String::from_utf8(output[..length].to_vec()).unwrap()
    }

    #[test]
    fn plans_activation_when_evidence_is_satisfied() {
        assert!(run(ELIGIBLE).contains("\"decision\":\"activation_planned\""));
    }

    #[test]
    fn unknown_evidence_follows_review_and_reject_policies() {
        let review = ELIGIBLE.replace("\"licence_evidence\":\"accepted\"", "\"licence_evidence\":\"unknown\"");
        assert!(run(&review).contains("review_required"));
        let reject = ELIGIBLE.replace("\"digest_evidence\":\"verified\"", "\"digest_evidence\":\"unknown\"")
            .replace("\"unknown_evidence_action\":\"review\"", "\"unknown_evidence_action\":\"reject\"");
        assert!(run(&reject).contains("verification_evidence_unknown"));
    }

    #[test]
    fn rejects_each_policy_failure_deterministically() {
        for (from, to, reason) in [
            ("\"target\":\"wasm32-wasip1\"", "\"target\":\"native\"", "target_not_allowed"),
            ("\"memory_required_bytes\":1024", "\"memory_required_bytes\":4096", "memory_budget_exceeded"),
            ("\"digest_evidence\":\"verified\"", "\"digest_evidence\":\"failed\"", "digest_verification_failed"),
            ("\"signature_evidence\":\"verified\"", "\"signature_evidence\":\"failed\"", "signature_verification_failed"),
            ("\"licence_evidence\":\"accepted\"", "\"licence_evidence\":\"rejected\"", "licence_not_accepted"),
            ("\"runtime_compatibility\":\"verified\"", "\"runtime_compatibility\":\"failed\"", "runtime_incompatible"),
        ] {
            assert!(run(&ELIGIBLE.replace(from, to)).contains(reason));
        }
    }

    #[test]
    fn rejects_malformed_oversized_and_duplicate_requests() {
        assert!(run("{}").contains("invalid_request"));
        assert!(run(&ELIGIBLE.replace("\"allowed_targets\":[\"wasm32-wasip1\"]", "\"allowed_targets\":[\"wasm32-wasip1\",]"))
            .contains("invalid_request"));
        assert!(run(&ELIGIBLE.replace("\"allowed_targets\":[\"wasm32-wasip1\"]", "\"allowed_targets\":[]"))
            .contains("invalid_request"));
        assert!(run(&ELIGIBLE.replace("\"model_ref\":", "\"model_ref\":\"duplicate\",\"model_ref\":"))
            .contains("invalid_request"));
        assert!(run(&std::string::String::from_utf8(vec![b'x'; 4097]).unwrap()).contains("input_limit_exceeded"));
    }

    #[test]
    fn string_array_rejects_invalid_members_and_accepts_valid_policy() {
        assert!(!string_array(br#"{"a":["valid"]}"#, b"\"a\"").invalid);
        for malformed in [
            b"{}".as_slice(), b"{\"a\":0}".as_slice(),
            b"{\"a\":[\"\"]}".as_slice(), b"{\"a\":[\"bad\\\\escape\"]}".as_slice(),
            b"{\"a\":[\"valid\",]}".as_slice(),
        ] {
            assert!(string_array(malformed, b"\"a\"").invalid);
        }
    }

    #[test]
    fn value_and_duplicate_scanners_handle_escaped_or_truncated_json() {
        assert!(value_after(br#"{"x":{"nested":[1,{"s":"}"}]},"y":true}"#, b"\"x\"").is_some());
        assert!(value_after(br#"{"x":"a\\\"b","y":1}"#, b"\"x\"").is_some());
        assert_eq!(value_after(b"{\"x\":[1,2", b"\"x\""), None);
        assert_eq!(value_after(b"{\"x\":\"unterminated", b"\"x\""), None);
        assert!(!unique_fields(b"{\"key\":1,\"key\":2}", &[b"\"key\""]));
        assert!(unique_fields(b"{\"other\":\"a\\\"b\",\"key\":1}", &[b"\"key\""]));
    }
}
