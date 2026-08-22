//! core.transition-action-status — action-item status state machine with governed emit.
//!
//! Evaluates whether `current_status → requested_status` is allowed under a
//! caller-supplied `transition_config`, optionally requiring the actor to be
//! the owner. On accepted transitions, emits
//! `core.action-item.status-transitioned@1.0.0` via `traverse_host::emit_event`.
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

#[cfg(not(test))]
#[link(wasm_import_module = "wasi_snapshot_preview1")]
unsafe extern "C" {
    fn fd_read(fd: u32, vectors: *const IoVecMut, count: usize, read: *mut usize) -> u32;
    fn fd_write(fd: u32, vectors: *const IoVec, count: usize, written: *mut usize) -> u32;
}

#[cfg(not(test))]
#[link(wasm_import_module = "traverse_host")]
unsafe extern "C" {
    fn emit_event(ptr: i32, len: i32) -> i32;
}

#[cfg(test)]
unsafe fn emit_event(_ptr: i32, _len: i32) -> i32 {
    0
}

static mut INPUT_BUF: [u8; 8192] = [0; 8192];
static mut OUTPUT_BUF: [u8; 4096] = [0; 4096];
static mut EVENT_BUF: [u8; 1024] = [0; 1024];

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    unsafe {
        let mut total = 0usize;
        loop {
            let vec = IoVecMut {
                buffer: INPUT_BUF.as_mut_ptr().add(total),
                length: INPUT_BUF.len() - total,
            };
            let mut n = 0usize;
            if fd_read(0, &vec, 1, &mut n) != 0 || n == 0 {
                break;
            }
            total += n;
            if total >= INPUT_BUF.len() {
                break;
            }
        }
        let out_len = evaluate(&INPUT_BUF[..total], &mut OUTPUT_BUF);
        let out = IoVec {
            buffer: OUTPUT_BUF.as_ptr(),
            length: out_len,
        };
        let mut written = 0usize;
        let _ = fd_write(1, &out, 1, &mut written);
    }
}

pub unsafe fn evaluate(input: &[u8], out: &mut [u8]) -> usize {
    let action_item_id = extract_string_at_depth(input, b"\"action_item_id\"", 1);
    let current = extract_string_at_depth(input, b"\"current_status\"", 1);
    let requested = extract_string_at_depth(input, b"\"requested_status\"", 1);
    let actor = extract_string_at_depth(input, b"\"actor_id\"", 1);
    let owner = extract_string_at_depth(input, b"\"owner_id\"", 1);
    let config = object_after_key(input, b"\"transition_config\"").unwrap_or(b"");

    if action_item_id.is_empty()
        || current.is_empty()
        || requested.is_empty()
        || actor.is_empty()
        || config.is_empty()
    {
        return write_result(
            out,
            false,
            if current.is_empty() { b"" } else { current },
            b"invalid_status",
            br#"["precondition failed: required fields missing"]"#,
        );
    }

    if !is_known_status(current) || !is_known_status(requested) {
        return write_result(
            out,
            false,
            current,
            b"invalid_status",
            br#"["status value is not a known action-item status"]"#,
        );
    }

    let owner_only = extract_bool(config, b"\"owner_only\"").unwrap_or(true);
    if owner_only {
        if owner.is_empty() || actor != owner {
            let mut trace = [0u8; 96];
            let mut t = 0usize;
            t = copy(
                &mut trace,
                t,
                br#"["owner_only=true and actor is not owner"]"#,
            );
            return write_result(out, false, current, b"not_owner", &trace[..t]);
        }
    }

    let transitions = object_after_key(config, b"\"allowed_transitions\"").unwrap_or(b"");
    if transitions.is_empty() {
        return write_result(
            out,
            false,
            current,
            b"illegal_transition",
            br#"["allowed_transitions missing"]"#,
        );
    }

    // Look up transitions[<current>] as a JSON array and see if <requested> is listed.
    let mut key_buf = [0u8; 64];
    let mut k = 0usize;
    k = copy(&mut key_buf, k, b"\"");
    k = copy(&mut key_buf, k, current);
    k = copy(&mut key_buf, k, b"\"");
    let Some(targets) = array_after_key(transitions, &key_buf[..k]) else {
        let mut trace = [0u8; 128];
        let mut t = 0usize;
        t = copy(&mut trace, t, b"[\"");
        t = copy(&mut trace, t, current);
        t = copy(&mut trace, t, b" has no transition list\"]");
        return write_result(out, false, current, b"illegal_transition", &trace[..t]);
    };

    if !array_contains_string(targets, requested) {
        let mut trace = [0u8; 160];
        let mut t = 0usize;
        t = copy(&mut trace, t, b"[\"");
        t = copy(&mut trace, t, current);
        t = copy(&mut trace, t, b" has no allowed transition to ");
        t = copy(&mut trace, t, requested);
        t = copy(&mut trace, t, b"\"]");
        return write_result(out, false, current, b"illegal_transition", &trace[..t]);
    }

    let mut trace = [0u8; 192];
    let mut t = 0usize;
    t = copy(&mut trace, t, b"[\"");
    t = copy(&mut trace, t, current);
    t = copy(&mut trace, t, " → ".as_bytes());
    t = copy(&mut trace, t, requested);
    t = copy(&mut trace, t, b" is allowed\"");
    if owner_only {
        t = copy(&mut trace, t, b", \"actor is owner\"");
    }
    t = copy(&mut trace, t, b"]");

    emit_status_transitioned(action_item_id, current, requested, actor);

    write_result(out, true, requested, b"ok", &trace[..t])
}

/// Build and emit the governed status-transitioned event. Best-effort: a
/// non-zero host status does not change the capability's own evaluation result.
unsafe fn emit_status_transitioned(
    action_item_id: &[u8],
    from_status: &[u8],
    to_status: &[u8],
    actor_id: &[u8],
) {
    let buf = &mut EVENT_BUF;
    let mut i = 0usize;
    i = copy(buf, i, br#"{"event_id":"core.action-item.status-transitioned","version":"1.0.0","payload":{"action_item_id":""#);
    i = copy(buf, i, action_item_id);
    i = copy(buf, i, br#"","from_status":""#);
    i = copy(buf, i, from_status);
    i = copy(buf, i, br#"","to_status":""#);
    i = copy(buf, i, to_status);
    i = copy(buf, i, br#"","actor_id":""#);
    i = copy(buf, i, actor_id);
    i = copy(buf, i, br#""}}"#);
    if i == 0 || i > buf.len() {
        return;
    }
    let _ = emit_event(buf.as_ptr() as i32, i as i32);
}

fn is_known_status(s: &[u8]) -> bool {
    matches!(
        s,
        b"open" | b"in_progress" | b"blocked" | b"snoozed" | b"done" | b"cancelled"
    )
}

fn write_result(
    out: &mut [u8],
    allowed: bool,
    new_status: &[u8],
    reason_code: &[u8],
    evaluation_trace: &[u8],
) -> usize {
    let mut i = 0usize;
    i = copy(out, i, b"{\"allowed\":");
    i = copy(out, i, if allowed { b"true" } else { b"false" });
    i = copy(out, i, b",\"new_status\":\"");
    i = copy(out, i, new_status);
    i = copy(out, i, b"\",\"reason_code\":\"");
    i = copy(out, i, reason_code);
    i = copy(out, i, b"\",\"evaluation_trace\":");
    i = copy(out, i, evaluation_trace);
    i = copy(out, i, b"}");
    i
}

fn object_after_key<'a>(hay: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let pos = find(hay, key)?;
    let after = &hay[pos + key.len()..];
    let colon = after.iter().position(|b| *b == b':')?;
    let mut rest = &after[colon + 1..];
    while rest.first() == Some(&b' ')
        || rest.first() == Some(&b'\n')
        || rest.first() == Some(&b'\t')
    {
        rest = &rest[1..];
    }
    if rest.first() != Some(&b'{') {
        return None;
    }
    let end = balanced_end(rest, b'{', b'}')?;
    Some(&rest[..=end])
}

fn array_after_key<'a>(hay: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    // Match `"name":` so value occurrences of the same string are not treated as keys.
    let mut keyed = [0u8; 96];
    if key.len() + 1 > keyed.len() {
        return None;
    }
    let mut k = 0usize;
    k = copy(&mut keyed, k, key);
    k = copy(&mut keyed, k, b":");
    let pos = find(hay, &keyed[..k])?;
    let mut rest = &hay[pos + k..];
    while rest.first() == Some(&b' ')
        || rest.first() == Some(&b'\n')
        || rest.first() == Some(&b'\t')
    {
        rest = &rest[1..];
    }
    if rest.first() != Some(&b'[') {
        return None;
    }
    let end = balanced_end(rest, b'[', b']')?;
    Some(&rest[..=end])
}

fn array_contains_string(array: &[u8], value: &[u8]) -> bool {
    let mut needle = [0u8; 80];
    let mut n = 0usize;
    n = copy(&mut needle, n, b"\"");
    n = copy(&mut needle, n, value);
    n = copy(&mut needle, n, b"\"");
    contains(array, &needle[..n])
}

fn balanced_end(s: &[u8], open: u8, close: u8) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_str = false;
    let mut i = 0usize;
    while i < s.len() {
        let b = s[i];
        if in_str {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' => in_str = true,
            x if x == open => depth += 1,
            x if x == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn extract_string_at_depth<'a>(hay: &'a [u8], key: &[u8], depth: i32) -> &'a [u8] {
    let Some(pos) = find_key_at_depth(hay, key, depth) else {
        return b"";
    };
    string_value_after(&hay[pos + key.len()..])
}

fn find_key_at_depth(hay: &[u8], key: &[u8], target_depth: i32) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_str = false;
    let mut i = 0usize;
    while i + key.len() <= hay.len() {
        let b = hay[i];
        if in_str {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' => {
                if depth == target_depth && hay[i..].starts_with(key) {
                    return Some(i);
                }
                in_str = true;
            }
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    None
}

fn string_value_after<'a>(after_key: &'a [u8]) -> &'a [u8] {
    let Some(colon) = after_key.iter().position(|b| *b == b':') else {
        return b"";
    };
    let mut rest = &after_key[colon + 1..];
    while rest.first() == Some(&b' ')
        || rest.first() == Some(&b'\n')
        || rest.first() == Some(&b'\t')
    {
        rest = &rest[1..];
    }
    if rest.first() != Some(&b'"') {
        return b"";
    }
    rest = &rest[1..];
    let Some(end) = rest.iter().position(|b| *b == b'"') else {
        return b"";
    };
    &rest[..end]
}

fn extract_bool(hay: &[u8], key: &[u8]) -> Option<bool> {
    let pos = find(hay, key)?;
    let after = &hay[pos + key.len()..];
    let colon = after.iter().position(|b| *b == b':')?;
    let mut rest = &after[colon + 1..];
    while rest.first() == Some(&b' ') {
        rest = &rest[1..];
    }
    if rest.starts_with(b"true") {
        Some(true)
    } else if rest.starts_with(b"false") {
        Some(false)
    } else {
        None
    }
}

fn copy(out: &mut [u8], at: usize, bytes: &[u8]) -> usize {
    let end = at + bytes.len();
    if end > out.len() {
        return at;
    }
    out[at..end].copy_from_slice(bytes);
    end
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}

#[cfg(test)]
mod catalog_coverage_tests {
    use super::*;

    fn run(input: &str) -> String {
        let mut out = vec![0u8; 65536];
        let n = unsafe { evaluate(input.as_bytes(), &mut out) };
        String::from_utf8_lossy(&out[..n]).into_owned()
    }

    #[test]
    fn use_case_01_happy() {
        let out = run("{\"action_item_id\":\"item-001\",\"current_status\":\"open\",\"requested_status\":\"in_progress\",\"actor_id\":\"user-ada\",\"owner_id\":\"user-ada\",\"transition_config\":{\"version\":\"1.0\",\"allowed_transitions\":{\"open\":[\"in_progress\",\"cancelled\",\"snoozed\"],\"in_progress\":[\"blocked\",\"done\",\"cancelled\",\"snoozed\"],\"blocked\":[\"in_progress\",\"cancelled\"],\"snoozed\":[\"open\",\"in_progress\",\"cancelled\"],\"done\":[],\"cancelled\":[]},\"owner_only\":true}}");
        assert!(
            out.contains("\"reason_code\":\"ok\""),
            "expected ok in {out}"
        );
    }

    #[test]
    fn use_case_02_sad() {
        let out = run("{\"action_item_id\":\"item-002\",\"current_status\":\"done\",\"requested_status\":\"open\",\"actor_id\":\"user-ada\",\"owner_id\":\"user-ada\",\"transition_config\":{\"version\":\"1.0\",\"allowed_transitions\":{\"open\":[\"in_progress\",\"cancelled\",\"snoozed\"],\"in_progress\":[\"blocked\",\"done\",\"cancelled\",\"snoozed\"],\"blocked\":[\"in_progress\",\"cancelled\"],\"snoozed\":[\"open\",\"in_progress\",\"cancelled\"],\"done\":[],\"cancelled\":[]},\"owner_only\":true}}");
        assert!(
            out.contains("\"reason_code\":\"illegal_transition\""),
            "expected illegal_transition in {out}"
        );
    }

    #[test]
    fn use_case_03_happy() {
        let out = run("{\"action_item_id\":\"item-003\",\"current_status\":\"open\",\"requested_status\":\"snoozed\",\"actor_id\":\"user-ada\",\"owner_id\":\"user-ada\",\"transition_config\":{\"version\":\"1.0\",\"allowed_transitions\":{\"open\":[\"in_progress\",\"cancelled\",\"snoozed\"],\"in_progress\":[\"blocked\",\"done\",\"cancelled\",\"snoozed\"],\"blocked\":[\"in_progress\",\"cancelled\"],\"snoozed\":[\"open\",\"in_progress\",\"cancelled\"],\"done\":[],\"cancelled\":[]},\"owner_only\":true}}");
        assert!(
            out.contains("\"reason_code\":\"ok\""),
            "expected ok in {out}"
        );
    }

    #[test]
    fn use_case_04_sad() {
        let out = run("{\"action_item_id\":\"item-004\",\"current_status\":\"open\",\"requested_status\":\"in_progress\",\"actor_id\":\"user-bob\",\"owner_id\":\"user-ada\",\"transition_config\":{\"version\":\"1.0\",\"allowed_transitions\":{\"open\":[\"in_progress\",\"cancelled\",\"snoozed\"],\"in_progress\":[\"blocked\",\"done\",\"cancelled\",\"snoozed\"],\"blocked\":[\"in_progress\",\"cancelled\"],\"snoozed\":[\"open\",\"in_progress\",\"cancelled\"],\"done\":[],\"cancelled\":[]},\"owner_only\":true}}");
        assert!(
            out.contains("\"reason_code\":\"not_owner\""),
            "expected not_owner in {out}"
        );
    }

    #[test]
    fn use_case_05_happy() {
        let out = run("{\"action_item_id\":\"item-005\",\"current_status\":\"in_progress\",\"requested_status\":\"blocked\",\"actor_id\":\"user-ada\",\"owner_id\":\"user-ada\",\"transition_config\":{\"version\":\"1.0\",\"allowed_transitions\":{\"open\":[\"in_progress\",\"cancelled\",\"snoozed\"],\"in_progress\":[\"blocked\",\"done\",\"cancelled\",\"snoozed\"],\"blocked\":[\"in_progress\",\"cancelled\"],\"snoozed\":[\"open\",\"in_progress\",\"cancelled\"],\"done\":[],\"cancelled\":[]},\"owner_only\":true}}");
        assert!(
            out.contains("\"reason_code\":\"ok\""),
            "expected ok in {out}"
        );
    }

    #[test]
    fn use_case_06_happy() {
        let out = run("{\"action_item_id\":\"item-006\",\"current_status\":\"in_progress\",\"requested_status\":\"done\",\"actor_id\":\"user-ada\",\"owner_id\":\"user-ada\",\"transition_config\":{\"version\":\"1.0\",\"allowed_transitions\":{\"open\":[\"in_progress\",\"cancelled\",\"snoozed\"],\"in_progress\":[\"blocked\",\"done\",\"cancelled\",\"snoozed\"],\"blocked\":[\"in_progress\",\"cancelled\"],\"snoozed\":[\"open\",\"in_progress\",\"cancelled\"],\"done\":[],\"cancelled\":[]},\"owner_only\":true}}");
        assert!(
            out.contains("\"reason_code\":\"ok\""),
            "expected ok in {out}"
        );
    }

    #[test]
    fn use_case_07_happy() {
        let out = run("{\"action_item_id\":\"item-007\",\"current_status\":\"blocked\",\"requested_status\":\"in_progress\",\"actor_id\":\"user-ada\",\"owner_id\":\"user-ada\",\"transition_config\":{\"version\":\"1.0\",\"allowed_transitions\":{\"open\":[\"in_progress\",\"cancelled\",\"snoozed\"],\"in_progress\":[\"blocked\",\"done\",\"cancelled\",\"snoozed\"],\"blocked\":[\"in_progress\",\"cancelled\"],\"snoozed\":[\"open\",\"in_progress\",\"cancelled\"],\"done\":[],\"cancelled\":[]},\"owner_only\":true}}");
        assert!(
            out.contains("\"reason_code\":\"ok\""),
            "expected ok in {out}"
        );
    }

    #[test]
    fn use_case_08_happy() {
        let out = run("{\"action_item_id\":\"item-008\",\"current_status\":\"snoozed\",\"requested_status\":\"open\",\"actor_id\":\"user-ada\",\"owner_id\":\"user-ada\",\"transition_config\":{\"version\":\"1.0\",\"allowed_transitions\":{\"open\":[\"in_progress\",\"cancelled\",\"snoozed\"],\"in_progress\":[\"blocked\",\"done\",\"cancelled\",\"snoozed\"],\"blocked\":[\"in_progress\",\"cancelled\"],\"snoozed\":[\"open\",\"in_progress\",\"cancelled\"],\"done\":[],\"cancelled\":[]},\"owner_only\":true}}");
        assert!(
            out.contains("\"reason_code\":\"ok\""),
            "expected ok in {out}"
        );
    }

    #[test]
    fn use_case_09_happy() {
        let out = run("{\"action_item_id\":\"item-009\",\"current_status\":\"open\",\"requested_status\":\"cancelled\",\"actor_id\":\"user-ada\",\"owner_id\":\"user-ada\",\"transition_config\":{\"version\":\"1.0\",\"allowed_transitions\":{\"open\":[\"in_progress\",\"cancelled\",\"snoozed\"],\"in_progress\":[\"blocked\",\"done\",\"cancelled\",\"snoozed\"],\"blocked\":[\"in_progress\",\"cancelled\"],\"snoozed\":[\"open\",\"in_progress\",\"cancelled\"],\"done\":[],\"cancelled\":[]},\"owner_only\":true}}");
        assert!(
            out.contains("\"reason_code\":\"ok\""),
            "expected ok in {out}"
        );
    }

    #[test]
    fn use_case_10_sad() {
        let out = run("{\"action_item_id\":\"item-010\",\"current_status\":\"cancelled\",\"requested_status\":\"open\",\"actor_id\":\"user-ada\",\"owner_id\":\"user-ada\",\"transition_config\":{\"version\":\"1.0\",\"allowed_transitions\":{\"open\":[\"in_progress\",\"cancelled\",\"snoozed\"],\"in_progress\":[\"blocked\",\"done\",\"cancelled\",\"snoozed\"],\"blocked\":[\"in_progress\",\"cancelled\"],\"snoozed\":[\"open\",\"in_progress\",\"cancelled\"],\"done\":[],\"cancelled\":[]},\"owner_only\":true}}");
        assert!(
            out.contains("\"reason_code\":\"illegal_transition\""),
            "expected illegal_transition in {out}"
        );
    }

    #[test]
    fn use_case_11_sad() {
        let out = run("{\"action_item_id\":\"item-011\",\"current_status\":\"\",\"requested_status\":\"open\",\"actor_id\":\"user-ada\",\"owner_id\":\"user-ada\",\"transition_config\":{\"version\":\"1.0\",\"allowed_transitions\":{\"open\":[\"in_progress\",\"cancelled\",\"snoozed\"],\"in_progress\":[\"blocked\",\"done\",\"cancelled\",\"snoozed\"],\"blocked\":[\"in_progress\",\"cancelled\"],\"snoozed\":[\"open\",\"in_progress\",\"cancelled\"],\"done\":[],\"cancelled\":[]},\"owner_only\":true}}");
        assert!(
            out.contains("\"reason_code\":\"invalid_status\""),
            "expected invalid_status in {out}"
        );
    }

    #[test]
    fn missing_required_fields_yields_invalid_status() {
        let out = run("{}");
        assert!(
            out.contains("\"reason_code\":\"invalid_status\""),
            "expected invalid_status in {out}"
        );
        assert!(out.contains("\"new_status\":\"\""));
    }

    #[test]
    fn unknown_requested_status_is_invalid() {
        let out = run("{\"action_item_id\":\"item-1\",\"current_status\":\"open\",\"requested_status\":\"vanished\",\"actor_id\":\"user-ada\",\"owner_id\":\"user-ada\",\"transition_config\":{\"allowed_transitions\":{\"open\":[\"vanished\"]},\"owner_only\":true}}");
        assert!(
            out.contains("\"reason_code\":\"invalid_status\""),
            "expected invalid_status in {out}"
        );
    }

    #[test]
    fn owner_only_false_allows_non_owner_actor() {
        let out = run("{\"action_item_id\":\"item-1\",\"current_status\":\"open\",\"requested_status\":\"in_progress\",\"actor_id\":\"user-bob\",\"owner_id\":\"user-ada\",\"transition_config\":{\"allowed_transitions\":{\"open\":[\"in_progress\"]},\"owner_only\":false}}");
        assert!(
            out.contains("\"reason_code\":\"ok\""),
            "expected ok in {out}"
        );
        assert!(!out.contains("actor is owner"));
    }

    #[test]
    fn missing_allowed_transitions_is_illegal_transition() {
        let out = run("{\"action_item_id\":\"item-1\",\"current_status\":\"open\",\"requested_status\":\"in_progress\",\"actor_id\":\"user-ada\",\"owner_id\":\"user-ada\",\"transition_config\":{\"owner_only\":true}}");
        assert!(
            out.contains("\"reason_code\":\"illegal_transition\""),
            "expected illegal_transition in {out}"
        );
        assert!(out.contains("allowed_transitions missing"));
    }

    #[test]
    fn status_with_no_transition_list_entry_is_illegal_transition() {
        let out = run("{\"action_item_id\":\"item-1\",\"current_status\":\"blocked\",\"requested_status\":\"open\",\"actor_id\":\"user-ada\",\"owner_id\":\"user-ada\",\"transition_config\":{\"allowed_transitions\":{\"open\":[\"in_progress\"]},\"owner_only\":true}}");
        assert!(
            out.contains("\"reason_code\":\"illegal_transition\""),
            "expected illegal_transition in {out}"
        );
        assert!(out.contains("has no transition list"));
    }

    #[test]
    fn object_after_key_handles_missing_and_non_object() {
        assert_eq!(object_after_key(b"{}", b"\"missing\""), None);
        assert_eq!(object_after_key(b"\"k\":5", b"\"k\""), None);
    }

    #[test]
    fn array_after_key_handles_missing_and_non_array() {
        assert_eq!(array_after_key(b"{}", b"\"missing\""), None);
        assert_eq!(array_after_key(b"\"k\":5", b"\"k\""), None);
    }

    #[test]
    fn extract_bool_handles_false_and_neither() {
        assert_eq!(extract_bool(b"\"k\":false", b"\"k\""), Some(false));
        assert_eq!(extract_bool(b"\"k\":maybe", b"\"k\""), None);
    }

    #[test]
    fn balanced_end_returns_none_when_unterminated() {
        assert_eq!(balanced_end(b"{\"a\":\"b\"", b'{', b'}'), None);
    }

    #[test]
    fn string_value_after_handles_missing_colon_quote_and_terminator() {
        assert_eq!(string_value_after(b"no colon"), b"");
        assert_eq!(string_value_after(b":not-a-quote"), b"");
        assert_eq!(string_value_after(b":\"unterminated"), b"");
    }

    #[test]
    fn array_contains_string_matches_exact_element() {
        assert!(array_contains_string(br#"["a","b"]"#, b"b"));
        assert!(!array_contains_string(br#"["a","b"]"#, b"c"));
    }
}
