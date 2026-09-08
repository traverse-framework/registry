//! support.ticket-sla-breached — decide whether a ticket SLA is breached.
//!
//! Inputs: ticket_id, due_at (unix seconds), now (unix seconds), priority.
//! Breach when now > due_at + priority_grace:
//!   urgent/high → 0s grace; normal → 300s; low → 900s.
//! Emits support.ticket.sla-breached@1.0.0 only when breached.
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
    let priority = extract_string_at_depth(input, b"\"priority\"", 1);
    let due_at = extract_i64(input, b"\"due_at\"");
    let now = extract_i64(input, b"\"now\"");

    if ticket_id.is_empty() || priority.is_empty() {
        return write_result(
            out,
            false,
            0,
            b"invalid_input",
            br#"["precondition failed: ticket_id and priority required"]"#,
        );
    }

    if !is_known_priority(priority) {
        return write_result(
            out,
            false,
            0,
            b"invalid_priority",
            br#"["priority must be low|normal|high|urgent"]"#,
        );
    }

    let (Some(due), Some(now_ts)) = (due_at, now) else {
        return write_result(
            out,
            false,
            0,
            b"invalid_input",
            br#"["precondition failed: due_at and now required as unix seconds"]"#,
        );
    };

    if due < 0 || now_ts < 0 {
        return write_result(
            out,
            false,
            0,
            b"invalid_input",
            br#"["due_at and now must be >= 0"]"#,
        );
    }

    let grace = priority_grace(priority);
    let effective_due = due.saturating_add(grace);
    if now_ts <= effective_due {
        let overdue = 0i64;
        return write_result(
            out,
            false,
            overdue,
            b"within_sla",
            br#"["now is at or before due_at + priority grace"]"#,
        );
    }

    let overdue = now_ts - effective_due;
    emit_sla_breached(ticket_id, due, now_ts, priority, overdue);
    write_result(
        out,
        true,
        overdue,
        b"breached",
        br#"["SLA breached after priority grace"]"#,
    )
}

fn priority_grace(priority: &[u8]) -> i64 {
    match priority {
        b"urgent" | b"high" => 0,
        b"normal" => 300,
        b"low" => 900,
        _ => 0,
    }
}

fn is_known_priority(p: &[u8]) -> bool {
    matches!(p, b"low" | b"normal" | b"high" | b"urgent")
}

unsafe fn emit_sla_breached(
    ticket_id: &[u8],
    due_at: i64,
    now: i64,
    priority: &[u8],
    overdue_seconds: i64,
) {
    let buf = &mut EVENT_BUF;
    let mut i = 0usize;
    i = copy(
        buf,
        i,
        br#"{"event_id":"support.ticket.sla-breached","version":"1.0.0","payload":{"ticket_id":""#,
    );
    i = copy(buf, i, ticket_id);
    i = copy(buf, i, br#"","due_at":"#);
    i = copy_i64(buf, i, due_at);
    i = copy(buf, i, br#","now":"#);
    i = copy_i64(buf, i, now);
    i = copy(buf, i, br#","priority":""#);
    i = copy(buf, i, priority);
    i = copy(buf, i, br#"","overdue_seconds":"#);
    i = copy_i64(buf, i, overdue_seconds);
    i = copy(buf, i, br#"}}"#);
    if i == 0 || i > buf.len() {
        return;
    }
    let _ = emit_event(buf.as_ptr() as i32, i as i32);
}

fn write_result(
    out: &mut [u8],
    breached: bool,
    overdue_seconds: i64,
    reason_code: &[u8],
    evaluation_trace: &[u8],
) -> usize {
    let mut i = 0usize;
    i = copy(out, i, b"{\"breached\":");
    i = copy(out, i, if breached { b"true" } else { b"false" });
    i = copy(out, i, b",\"overdue_seconds\":");
    i = copy_i64(out, i, overdue_seconds);
    i = copy(out, i, b",\"reason_code\":\"");
    i = copy(out, i, reason_code);
    i = copy(out, i, b"\",\"evaluation_trace\":");
    i = copy(out, i, evaluation_trace);
    i = copy(out, i, b"}");
    i
}

fn extract_i64(hay: &[u8], key: &[u8]) -> Option<i64> {
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
    let mut neg = false;
    if rest.first() == Some(&b'-') {
        neg = true;
        rest = &rest[1..];
    }
    let mut val: i64 = 0;
    let mut any = false;
    for &b in rest {
        if b < b'0' || b > b'9' {
            break;
        }
        any = true;
        val = val.saturating_mul(10).saturating_add(i64::from(b - b'0'));
    }
    if !any {
        return None;
    }
    Some(if neg { -val } else { val })
}

fn copy_i64(out: &mut [u8], at: usize, mut n: i64) -> usize {
    if n == 0 {
        return copy(out, at, b"0");
    }
    let mut neg = false;
    if n < 0 {
        neg = true;
        n = -n;
    }
    let mut digits = [0u8; 24];
    let mut d = 0usize;
    while n > 0 {
        digits[d] = b'0' + (n % 10) as u8;
        d += 1;
        n /= 10;
    }
    let mut i = at;
    if neg {
        i = copy(out, i, b"-");
    }
    while d > 0 {
        d -= 1;
        if i >= out.len() {
            return at;
        }
        out[i] = digits[d];
        i += 1;
    }
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

    fn run(input: &str) -> String {
        let mut out = vec![0u8; 65536];
        let n = unsafe { evaluate(input.as_bytes(), &mut out) };
        String::from_utf8_lossy(&out[..n]).into_owned()
    }

    #[test]
    fn high_priority_breached_immediately() {
        let out = run(r#"{"ticket_id":"t-1","due_at":1000,"now":1001,"priority":"high"}"#);
        assert!(out.contains("\"reason_code\":\"breached\""), "{out}");
        assert!(out.contains("\"breached\":true"));
        assert!(out.contains("\"overdue_seconds\":1"));
    }

    #[test]
    fn normal_within_grace() {
        let out = run(r#"{"ticket_id":"t-2","due_at":1000,"now":1200,"priority":"normal"}"#);
        assert!(out.contains("\"reason_code\":\"within_sla\""), "{out}");
        assert!(out.contains("\"breached\":false"));
    }

    #[test]
    fn normal_past_grace_breached() {
        let out = run(r#"{"ticket_id":"t-3","due_at":1000,"now":1400,"priority":"normal"}"#);
        assert!(out.contains("\"reason_code\":\"breached\""), "{out}");
        assert!(out.contains("\"overdue_seconds\":100"));
    }

    #[test]
    fn low_grace_longer() {
        let out = run(r#"{"ticket_id":"t-4","due_at":1000,"now":1800,"priority":"low"}"#);
        assert!(out.contains("\"reason_code\":\"within_sla\""), "{out}");
    }

    #[test]
    fn rejects_bad_priority() {
        let out = run(r#"{"ticket_id":"t-5","due_at":1000,"now":2000,"priority":"critical"}"#);
        assert!(
            out.contains("\"reason_code\":\"invalid_priority\""),
            "{out}"
        );
    }

    #[test]
    fn rejects_missing() {
        let out = run("{}");
        assert!(out.contains("\"reason_code\":\"invalid_input\""), "{out}");
    }

    #[test]
    fn rejects_missing_timestamps() {
        let out = run(r#"{"ticket_id":"t-6","priority":"high"}"#);
        assert!(out.contains("\"reason_code\":\"invalid_input\""), "{out}");
    }

    #[test]
    fn rejects_negative_timestamps() {
        let out = run(r#"{"ticket_id":"t-7","due_at":-1,"now":100,"priority":"high"}"#);
        assert!(out.contains("\"reason_code\":\"invalid_input\""), "{out}");
        let out2 = run(r#"{"ticket_id":"t-8","due_at":100,"now":-5,"priority":"high"}"#);
        assert!(out2.contains("\"reason_code\":\"invalid_input\""), "{out2}");
    }

    #[test]
    fn urgent_priority_breached() {
        let out = run(r#"{"ticket_id":"t-9","due_at":1000,"now":1001,"priority":"urgent"}"#);
        assert!(out.contains("\"reason_code\":\"breached\""), "{out}");
    }

    #[test]
    fn priority_grace_unknown_returns_zero() {
        assert_eq!(priority_grace(b"weird"), 0);
    }

    #[test]
    fn extract_i64_whitespace_negative_and_non_digit() {
        assert_eq!(extract_i64(b"\"due_at\":\n\t 12", b"\"due_at\""), Some(12));
        assert_eq!(extract_i64(b"\"due_at\": -7", b"\"due_at\""), Some(-7));
        assert_eq!(extract_i64(b"\"due_at\":", b"\"due_at\""), None);
        assert_eq!(extract_i64(b"\"due_at\": true", b"\"due_at\""), None);
    }

    #[test]
    fn copy_i64_negative_and_overflow() {
        let mut buf = [0u8; 16];
        let n = copy_i64(&mut buf, 0, -42);
        assert_eq!(&buf[..n], b"-42");
        let mut tiny = [0u8; 1];
        assert_eq!(copy_i64(&mut tiny, 0, 12345), 0);
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
        let with_escape = br#"{"note":"say \"hi\"","ticket_id":"t1","priority":"high"}"#;
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
