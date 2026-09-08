//! support.ticket-status-changed — built-in ticket status state machine.
//!
//! Allowed graph (fixed, not caller-supplied):
//!   open → in_progress | waiting_customer | resolved | cancelled
//!   in_progress → waiting_customer | resolved | cancelled
//!   waiting_customer → in_progress | resolved | cancelled
//!   resolved → closed | open
//!   closed → []
//!   cancelled → []
//! On accepted transitions, emits support.ticket.status-changed@1.0.0.
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
    let ticket_id = extract_string_at_depth(input, b"\"ticket_id\"", 1);
    let current = extract_string_at_depth(input, b"\"current_status\"", 1);
    let requested = extract_string_at_depth(input, b"\"requested_status\"", 1);
    let actor = extract_string_at_depth(input, b"\"actor_id\"", 1);

    if ticket_id.is_empty() || current.is_empty() || requested.is_empty() || actor.is_empty() {
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
            br#"["status value is not a known ticket status"]"#,
        );
    }

    if !transition_allowed(current, requested) {
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

    emit_ticket_status_changed(ticket_id, current, requested, actor);
    write_result(out, true, requested, b"ok", &trace[..t])
}

fn transition_allowed(from: &[u8], to: &[u8]) -> bool {
    match from {
        b"open" => matches!(
            to,
            b"in_progress" | b"waiting_customer" | b"resolved" | b"cancelled"
        ),
        b"in_progress" => matches!(to, b"waiting_customer" | b"resolved" | b"cancelled"),
        b"waiting_customer" => matches!(to, b"in_progress" | b"resolved" | b"cancelled"),
        b"resolved" => matches!(to, b"closed" | b"open"),
        b"closed" | b"cancelled" => false,
        _ => false,
    }
}

fn is_known_status(s: &[u8]) -> bool {
    matches!(
        s,
        b"open" | b"in_progress" | b"waiting_customer" | b"resolved" | b"closed" | b"cancelled"
    )
}

unsafe fn emit_ticket_status_changed(
    ticket_id: &[u8],
    from_status: &[u8],
    to_status: &[u8],
    actor_id: &[u8],
) {
    let buf = &mut EVENT_BUF;
    let mut i = 0usize;
    i = copy(
        buf,
        i,
        br#"{"event_id":"support.ticket.status-changed","version":"1.0.0","payload":{"ticket_id":""#,
    );
    i = copy(buf, i, ticket_id);
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

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(input: &str) -> String {
        let mut out = vec![0u8; 65536];
        let n = unsafe { evaluate(input.as_bytes(), &mut out) };
        String::from_utf8_lossy(&out[..n]).into_owned()
    }

    #[test]
    fn happy_open_to_in_progress() {
        let out = run(
            r#"{"ticket_id":"t-1","current_status":"open","requested_status":"in_progress","actor_id":"agent-1"}"#,
        );
        assert!(out.contains("\"reason_code\":\"ok\""), "{out}");
    }

    #[test]
    fn happy_resolved_reopen() {
        let out = run(
            r#"{"ticket_id":"t-2","current_status":"resolved","requested_status":"open","actor_id":"agent-1"}"#,
        );
        assert!(out.contains("\"reason_code\":\"ok\""), "{out}");
    }

    #[test]
    fn rejects_closed_reopen() {
        let out = run(
            r#"{"ticket_id":"t-3","current_status":"closed","requested_status":"open","actor_id":"agent-1"}"#,
        );
        assert!(
            out.contains("\"reason_code\":\"illegal_transition\""),
            "{out}"
        );
    }

    #[test]
    fn rejects_open_to_closed_skip() {
        let out = run(
            r#"{"ticket_id":"t-4","current_status":"open","requested_status":"closed","actor_id":"agent-1"}"#,
        );
        assert!(
            out.contains("\"reason_code\":\"illegal_transition\""),
            "{out}"
        );
    }

    #[test]
    fn rejects_unknown() {
        let out = run(
            r#"{"ticket_id":"t-5","current_status":"open","requested_status":"escalated","actor_id":"agent-1"}"#,
        );
        assert!(out.contains("\"reason_code\":\"invalid_status\""), "{out}");
    }

    #[test]
    fn missing_fields() {
        let out = run("{}");
        assert!(out.contains("\"reason_code\":\"invalid_status\""), "{out}");
    }

    #[test]
    fn rejects_cancelled_reopen() {
        let out = run(
            r#"{"ticket_id":"t-6","current_status":"cancelled","requested_status":"open","actor_id":"agent-1"}"#,
        );
        assert!(
            out.contains("\"reason_code\":\"illegal_transition\""),
            "{out}"
        );
    }

    #[test]
    fn happy_in_progress_and_waiting_customer() {
        let out = run(
            r#"{"ticket_id":"t-7","current_status":"in_progress","requested_status":"waiting_customer","actor_id":"agent-1"}"#,
        );
        assert!(out.contains("\"reason_code\":\"ok\""), "{out}");
        let out2 = run(
            r#"{"ticket_id":"t-8","current_status":"waiting_customer","requested_status":"resolved","actor_id":"agent-1"}"#,
        );
        assert!(out2.contains("\"reason_code\":\"ok\""), "{out2}");
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
        let with_escape = br#"{"note":"say \"hi\"","ticket_id":"t1","current_status":"open"}"#;
        assert_eq!(
            extract_string_at_depth(with_escape, b"\"ticket_id\"", 1),
            b"t1"
        );
        let nested = br#"{"wrap":{"x":1},"ticket_id":"t2"}"#;
        assert_eq!(extract_string_at_depth(nested, b"\"ticket_id\"", 1), b"t2");
        let with_array = br#"{"tags":["a","b"],"ticket_id":"t3"}"#;
        assert_eq!(
            extract_string_at_depth(with_array, b"\"ticket_id\"", 1),
            b"t3"
        );
    }

    #[test]
    fn copy_overflow_returns_at() {
        let mut tiny = [0u8; 2];
        assert_eq!(copy(&mut tiny, 0, b"hello"), 0);
    }
}
