//! commerce.order-status-changed — order status transition under caller config.
//!
//! Evaluates current_status → requested_status against transition_config.allowed_transitions.
//! On accepted transitions, emits commerce.order.status-changed@1.0.0.
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
    let order_id = extract_string_at_depth(input, b"\"order_id\"", 1);
    let current = extract_string_at_depth(input, b"\"current_status\"", 1);
    let requested = extract_string_at_depth(input, b"\"requested_status\"", 1);
    let actor = extract_string_at_depth(input, b"\"actor_id\"", 1);
    let config = object_after_key(input, b"\"transition_config\"").unwrap_or(b"");

    if order_id.is_empty()
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
            br#"["status value is not a known order status"]"#,
        );
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

    let mut trace = [0u8; 160];
    let mut t = 0usize;
    t = copy(&mut trace, t, b"[\"");
    t = copy(&mut trace, t, current);
    t = copy(&mut trace, t, " → ".as_bytes());
    t = copy(&mut trace, t, requested);
    t = copy(&mut trace, t, b" is allowed\"]");

    emit_order_status_changed(order_id, current, requested, actor);
    write_result(out, true, requested, b"ok", &trace[..t])
}

unsafe fn emit_order_status_changed(
    order_id: &[u8],
    from_status: &[u8],
    to_status: &[u8],
    actor_id: &[u8],
) {
    let buf = &mut EVENT_BUF;
    let mut i = 0usize;
    i = copy(
        buf,
        i,
        br#"{"event_id":"commerce.order.status-changed","version":"1.0.0","payload":{"order_id":""#,
    );
    i = copy(buf, i, order_id);
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
        b"pending"
            | b"confirmed"
            | b"fulfilled"
            | b"shipped"
            | b"delivered"
            | b"cancelled"
            | b"returned"
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
mod tests {
    use super::*;

    const CFG: &str = r#"{"version":"1.0","allowed_transitions":{"pending":["confirmed","cancelled"],"confirmed":["fulfilled","cancelled"],"fulfilled":["shipped","cancelled"],"shipped":["delivered","returned"],"delivered":[],"cancelled":[],"returned":[]}}"#;

    fn run(input: &str) -> String {
        let mut out = vec![0u8; 65536];
        let n = unsafe { evaluate(input.as_bytes(), &mut out) };
        String::from_utf8_lossy(&out[..n]).into_owned()
    }

    #[test]
    fn happy_pending_to_confirmed() {
        let input = format!(
            r#"{{"order_id":"ord-1","current_status":"pending","requested_status":"confirmed","actor_id":"ops-1","transition_config":{CFG}}}"#
        );
        let out = run(&input);
        assert!(out.contains("\"reason_code\":\"ok\""), "{out}");
        assert!(out.contains("\"new_status\":\"confirmed\""));
    }

    #[test]
    fn rejects_illegal_jump() {
        let input = format!(
            r#"{{"order_id":"ord-2","current_status":"pending","requested_status":"delivered","actor_id":"ops-1","transition_config":{CFG}}}"#
        );
        let out = run(&input);
        assert!(
            out.contains("\"reason_code\":\"illegal_transition\""),
            "{out}"
        );
    }

    #[test]
    fn rejects_unknown_status() {
        let input = format!(
            r#"{{"order_id":"ord-3","current_status":"pending","requested_status":"lost","actor_id":"ops-1","transition_config":{CFG}}}"#
        );
        let out = run(&input);
        assert!(out.contains("\"reason_code\":\"invalid_status\""), "{out}");
    }

    #[test]
    fn rejects_terminal_cancelled() {
        let input = format!(
            r#"{{"order_id":"ord-4","current_status":"cancelled","requested_status":"pending","actor_id":"ops-1","transition_config":{CFG}}}"#
        );
        let out = run(&input);
        assert!(
            out.contains("\"reason_code\":\"illegal_transition\""),
            "{out}"
        );
    }

    #[test]
    fn happy_shipped_to_delivered() {
        let input = format!(
            r#"{{"order_id":"ord-5","current_status":"shipped","requested_status":"delivered","actor_id":"ops-1","transition_config":{CFG}}}"#
        );
        let out = run(&input);
        assert!(out.contains("\"reason_code\":\"ok\""), "{out}");
    }

    #[test]
    fn missing_fields() {
        let out = run("{}");
        assert!(out.contains("\"reason_code\":\"invalid_status\""), "{out}");
    }

    #[test]
    fn rejects_missing_allowed_transitions() {
        let out = run(
            r#"{"order_id":"ord-6","current_status":"pending","requested_status":"confirmed","actor_id":"ops-1","transition_config":{"version":"1.0"}}"#,
        );
        assert!(
            out.contains("\"reason_code\":\"illegal_transition\""),
            "{out}"
        );
        assert!(out.contains("allowed_transitions missing"), "{out}");
    }

    #[test]
    fn rejects_status_with_no_transition_list() {
        let cfg = r#"{"version":"1.0","allowed_transitions":{"pending":["confirmed"]}}"#;
        let input = format!(
            r#"{{"order_id":"ord-7","current_status":"confirmed","requested_status":"fulfilled","actor_id":"ops-1","transition_config":{cfg}}}"#
        );
        let out = run(&input);
        assert!(
            out.contains("\"reason_code\":\"illegal_transition\""),
            "{out}"
        );
        assert!(out.contains("has no transition list"), "{out}");
    }

    #[test]
    fn object_and_array_helpers_edge_cases() {
        assert_eq!(object_after_key(b"{}", b"\"missing\""), None);
        assert_eq!(object_after_key(br#"{"k":5}"#, b"\"k\""), None);
        assert!(object_after_key(b"\"k\":\n\t{\"a\":1}", b"\"k\"").is_some());
        assert_eq!(array_after_key(br#"{"k":5}"#, b"\"k\""), None);
        assert!(array_after_key(b"\"pending\":\n\t[\"confirmed\"]", b"\"pending\"").is_some());
        let long = [b'a'; 100];
        assert_eq!(array_after_key(b"{}", &long), None);
    }

    #[test]
    fn balanced_end_handles_escape_and_unterminated() {
        assert_eq!(balanced_end(b"{\"a\":\"b\"", b'{', b'}'), None);
        assert!(balanced_end(b"{\"a\":\"say \\\"hi\\\"\"}", b'{', b'}').is_some());
    }

    #[test]
    fn string_value_after_edge_cases() {
        assert_eq!(string_value_after(b"no colon"), b"");
        assert_eq!(string_value_after(b":not-a-quote"), b"");
        assert_eq!(string_value_after(b":\"unterminated"), b"");
        assert_eq!(string_value_after(b": \"ok\""), b"ok");
        assert_eq!(string_value_after(b":\n\t\"ok\""), b"ok");
    }

    #[test]
    fn find_key_at_depth_handles_escapes_nesting_and_arrays() {
        let with_escape = br#"{"note":"say \"hi\"","order_id":"o1","current_status":"pending"}"#;
        assert_eq!(
            extract_string_at_depth(with_escape, b"\"order_id\"", 1),
            b"o1"
        );
        let nested = br#"{"wrap":{"x":1},"order_id":"o2"}"#;
        assert_eq!(extract_string_at_depth(nested, b"\"order_id\"", 1), b"o2");
        let with_array = br#"{"tags":["a","b"],"order_id":"o3"}"#;
        assert_eq!(
            extract_string_at_depth(with_array, b"\"order_id\"", 1),
            b"o3"
        );
    }

    #[test]
    fn copy_overflow_returns_at() {
        let mut tiny = [0u8; 2];
        assert_eq!(copy(&mut tiny, 0, b"hello"), 0);
    }
}
