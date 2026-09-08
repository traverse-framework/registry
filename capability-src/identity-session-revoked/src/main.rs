//! identity.session-revoked — validate a session revocation request and emit.
//!
//! Inputs: session_id, principal_id, reason (enum), actor_id.
//! Reasons: user_logout | admin_revoke | security_compromise | idle_timeout | password_change.
//! On success emits identity.session.revoked@1.0.0.
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
    let session_id = extract_string_at_depth(input, b"\"session_id\"", 1);
    let principal_id = extract_string_at_depth(input, b"\"principal_id\"", 1);
    let reason = extract_string_at_depth(input, b"\"reason\"", 1);
    let actor_id = extract_string_at_depth(input, b"\"actor_id\"", 1);

    if session_id.is_empty() || principal_id.is_empty() || reason.is_empty() || actor_id.is_empty()
    {
        return write_result(
            out,
            false,
            b"invalid_input",
            br#"["precondition failed: required fields missing"]"#,
        );
    }

    if !is_known_reason(reason) {
        return write_result(
            out,
            false,
            b"invalid_reason",
            br#"["reason must be a known revocation reason"]"#,
        );
    }

    // Admin/security revocations may be performed by a different actor than the
    // principal; user_logout / password_change / idle_timeout require actor == principal.
    if matches!(
        reason,
        b"user_logout" | b"password_change" | b"idle_timeout"
    ) && actor_id != principal_id
    {
        return write_result(
            out,
            false,
            b"actor_mismatch",
            br#"["self-service reasons require actor_id == principal_id"]"#,
        );
    }

    emit_session_revoked(session_id, principal_id, reason, actor_id);

    let mut trace = [0u8; 128];
    let mut t = 0usize;
    t = copy(&mut trace, t, b"[\"");
    t = copy(&mut trace, t, reason);
    t = copy(&mut trace, t, b" accepted\"]");
    write_result(out, true, b"ok", &trace[..t])
}

fn is_known_reason(r: &[u8]) -> bool {
    matches!(
        r,
        b"user_logout"
            | b"admin_revoke"
            | b"security_compromise"
            | b"idle_timeout"
            | b"password_change"
    )
}

unsafe fn emit_session_revoked(
    session_id: &[u8],
    principal_id: &[u8],
    reason: &[u8],
    actor_id: &[u8],
) {
    let buf = &mut EVENT_BUF;
    let mut i = 0usize;
    i = copy(
        buf,
        i,
        br#"{"event_id":"identity.session.revoked","version":"1.0.0","payload":{"session_id":""#,
    );
    i = copy(buf, i, session_id);
    i = copy(buf, i, br#"","principal_id":""#);
    i = copy(buf, i, principal_id);
    i = copy(buf, i, br#"","reason":""#);
    i = copy(buf, i, reason);
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
    accepted: bool,
    reason_code: &[u8],
    evaluation_trace: &[u8],
) -> usize {
    let mut i = 0usize;
    i = copy(out, i, b"{\"accepted\":");
    i = copy(out, i, if accepted { b"true" } else { b"false" });
    i = copy(out, i, b",\"reason_code\":\"");
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
    fn happy_user_logout() {
        let out = run(
            r#"{"session_id":"sess-1","principal_id":"user-ada","reason":"user_logout","actor_id":"user-ada"}"#,
        );
        assert!(out.contains("\"reason_code\":\"ok\""), "{out}");
    }

    #[test]
    fn happy_admin_revoke_other() {
        let out = run(
            r#"{"session_id":"sess-2","principal_id":"user-ada","reason":"admin_revoke","actor_id":"admin-1"}"#,
        );
        assert!(out.contains("\"reason_code\":\"ok\""), "{out}");
    }

    #[test]
    fn rejects_logout_by_other() {
        let out = run(
            r#"{"session_id":"sess-3","principal_id":"user-ada","reason":"user_logout","actor_id":"admin-1"}"#,
        );
        assert!(out.contains("\"reason_code\":\"actor_mismatch\""), "{out}");
    }

    #[test]
    fn rejects_bad_reason() {
        let out = run(
            r#"{"session_id":"sess-4","principal_id":"user-ada","reason":"ban","actor_id":"user-ada"}"#,
        );
        assert!(out.contains("\"reason_code\":\"invalid_reason\""), "{out}");
    }

    #[test]
    fn happy_security_compromise() {
        let out = run(
            r#"{"session_id":"sess-5","principal_id":"user-ada","reason":"security_compromise","actor_id":"sec-bot"}"#,
        );
        assert!(out.contains("\"reason_code\":\"ok\""), "{out}");
    }

    #[test]
    fn missing_fields() {
        let out = run("{}");
        assert!(out.contains("\"reason_code\":\"invalid_input\""), "{out}");
    }

    #[test]
    fn happy_password_change_and_idle_timeout() {
        let out = run(
            r#"{"session_id":"sess-6","principal_id":"user-ada","reason":"password_change","actor_id":"user-ada"}"#,
        );
        assert!(out.contains("\"reason_code\":\"ok\""), "{out}");
        let out2 = run(
            r#"{"session_id":"sess-7","principal_id":"user-ada","reason":"idle_timeout","actor_id":"user-ada"}"#,
        );
        assert!(out2.contains("\"reason_code\":\"ok\""), "{out2}");
    }

    #[test]
    fn rejects_password_change_by_other() {
        let out = run(
            r#"{"session_id":"sess-8","principal_id":"user-ada","reason":"password_change","actor_id":"admin-1"}"#,
        );
        assert!(out.contains("\"reason_code\":\"actor_mismatch\""), "{out}");
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
        let with_escape = br#"{"note":"say \"hi\"","session_id":"s1","principal_id":"p1"}"#;
        assert_eq!(
            extract_string_at_depth(with_escape, b"\"session_id\"", 1),
            b"s1"
        );
        let nested = br#"{"wrap":{"x":1},"session_id":"s2"}"#;
        assert_eq!(extract_string_at_depth(nested, b"\"session_id\"", 1), b"s2");
        let with_array = br#"{"tags":["a","b"],"session_id":"s3"}"#;
        assert_eq!(
            extract_string_at_depth(with_array, b"\"session_id\"", 1),
            b"s3"
        );
    }

    #[test]
    fn copy_overflow_returns_at() {
        let mut tiny = [0u8; 2];
        assert_eq!(copy(&mut tiny, 0, b"hello"), 0);
    }
}
