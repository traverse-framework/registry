//! identity.principal-session — put/get/delete a durable session via host state.
//!
//! Relative state key is fixed schema property `session`. Resource id lives inside the value
//! (`session_id`, `principal_id`, `issued_at`, `scopes`). Calls traverse_host::{state_put,state_get,state_delete}.
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
    fn state_get(ptr: i32, len: i32) -> i32;
    fn state_put(ptr: i32, len: i32) -> i32;
    fn state_delete(ptr: i32, len: i32) -> i32;
}

#[cfg(test)]
thread_local! {
    static TEST_GET_RESPONSE: std::cell::RefCell<Vec<u8>> =
        std::cell::RefCell::new(br#"{"found":false}"#.to_vec());
    static TEST_HOST_RC: std::cell::RefCell<i32> = std::cell::RefCell::new(0);
}

#[cfg(test)]
unsafe fn state_get(_ptr: i32, _len: i32) -> i32 {
    // Native host pointers do not fit in i32 (wasm ABI). Write into STATE_OUT directly.
    let rc = TEST_HOST_RC.with(|c| *c.borrow());
    if rc != 0 {
        return rc;
    }
    TEST_GET_RESPONSE.with(|resp| {
        let bytes = resp.borrow();
        if bytes.len() > STATE_OUT.len() {
            return -1;
        }
        for b in STATE_OUT.iter_mut() {
            *b = 0;
        }
        STATE_OUT[..bytes.len()].copy_from_slice(&bytes);
        0
    })
}

#[cfg(test)]
unsafe fn state_put(_ptr: i32, _len: i32) -> i32 {
    TEST_HOST_RC.with(|c| *c.borrow())
}

#[cfg(test)]
unsafe fn state_delete(_ptr: i32, _len: i32) -> i32 {
    TEST_HOST_RC.with(|c| *c.borrow())
}

const STATE_KEY: &[u8] = b"session";

static mut INPUT_BUF: [u8; 8192] = [0; 8192];
static mut OUTPUT_BUF: [u8; 8192] = [0; 8192];
static mut STATE_REQ: [u8; 8192] = [0; 8192];
static mut STATE_OUT: [u8; 4096] = [0; 4096];

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
    let action = extract_string_at_depth(input, b"\"action\"", 1);
    if action.is_empty() {
        return write_result(
            out,
            false,
            b"invalid_input",
            None,
            None,
            br#"["precondition failed: action required"]"#,
        );
    }

    if action == b"put" {
        return do_put(input, out);
    }
    if action == b"get" {
        return do_get(out);
    }
    if action == b"delete" {
        return do_delete(out);
    }
    write_result(
        out,
        false,
        b"invalid_action",
        None,
        None,
        br#"["action must be get, put, or delete"]"#,
    )
}

unsafe fn do_put(input: &[u8], out: &mut [u8]) -> usize {
    let Some(session) = extract_object_at_depth(input, b"\"session\"", 1) else {
        return write_result(
            out,
            false,
            b"invalid_input",
            None,
            None,
            br#"["precondition failed: session object required for put"]"#,
        );
    };
    let session_id = extract_string_at_depth(session, b"\"session_id\"", 1);
    let principal_id = extract_string_at_depth(session, b"\"principal_id\"", 1);
    let issued_at = extract_string_at_depth(session, b"\"issued_at\"", 1);
    let scopes = extract_array_at_depth(session, b"\"scopes\"", 1);
    if session_id.is_empty()
        || principal_id.is_empty()
        || issued_at.is_empty()
        || scopes.is_none()
    {
        return write_result(
            out,
            false,
            b"invalid_input",
            None,
            None,
            br#"["precondition failed: session.session_id/principal_id/issued_at/scopes required"]"#,
        );
    }

    let req = &mut STATE_REQ;
    let mut i = 0usize;
    i = copy(req, i, br#"{"key":""#);
    i = copy(req, i, STATE_KEY);
    i = copy(req, i, br#"","value":"#);
    i = copy(req, i, session);
    i = copy(req, i, b"}");
    let rc = state_put(req.as_ptr() as i32, i as i32);
    if rc != 0 {
        return write_result(
            out,
            false,
            b"host_error",
            None,
            None,
            br#"["state_put failed"]"#,
        );
    }
    write_result(
        out,
        true,
        b"ok",
        Some(true),
        Some(session),
        br#"["put session"]"#,
    )
}

unsafe fn do_get(out: &mut [u8]) -> usize {
    let req = &mut STATE_REQ;
    let mut i = 0usize;
    i = copy(req, i, br#"{"key":""#);
    i = copy(req, i, STATE_KEY);
    i = copy(req, i, br#"","out_ptr":"#);
    i = copy_usize(req, i, STATE_OUT.as_ptr() as usize);
    i = copy(req, i, br#","out_max":"#);
    i = copy_usize(req, i, STATE_OUT.len());
    i = copy(req, i, b"}");
    for b in STATE_OUT.iter_mut() {
        *b = 0;
    }
    let rc = state_get(req.as_ptr() as i32, i as i32);
    if rc != 0 {
        return write_result(
            out,
            false,
            b"host_error",
            None,
            None,
            br#"["state_get failed"]"#,
        );
    }
    let resp = trim_c_str(&STATE_OUT);
    if find(resp, b"\"found\":true").is_some() {
        if let Some(value) = extract_object_at_depth(resp, b"\"value\"", 1) {
            return write_result(
                out,
                true,
                b"ok",
                Some(true),
                Some(value),
                br#"["got session"]"#,
            );
        }
        return write_result(
            out,
            false,
            b"host_error",
            Some(true),
            None,
            br#"["state_get found without value object"]"#,
        );
    }
    write_result(
        out,
        false,
        b"not_found",
        Some(false),
        None,
        br#"["session absent"]"#,
    )
}

unsafe fn do_delete(out: &mut [u8]) -> usize {
    let req = &mut STATE_REQ;
    let mut i = 0usize;
    i = copy(req, i, br#"{"key":""#);
    i = copy(req, i, STATE_KEY);
    i = copy(req, i, b"\"}");
    let rc = state_delete(req.as_ptr() as i32, i as i32);
    if rc != 0 {
        return write_result(
            out,
            false,
            b"host_error",
            None,
            None,
            br#"["state_delete failed"]"#,
        );
    }
    write_result(
        out,
        true,
        b"ok",
        None,
        None,
        br#"["deleted session"]"#,
    )
}

fn write_result(
    out: &mut [u8],
    accepted: bool,
    reason_code: &[u8],
    found: Option<bool>,
    session: Option<&[u8]>,
    evaluation_trace: &[u8],
) -> usize {
    let mut i = 0usize;
    i = copy(out, i, b"{\"accepted\":");
    i = copy(out, i, if accepted { b"true" } else { b"false" });
    i = copy(out, i, b",\"reason_code\":\"");
    i = copy(out, i, reason_code);
    i = copy(out, i, b"\"");
    if let Some(f) = found {
        i = copy(out, i, b",\"found\":");
        i = copy(out, i, if f { b"true" } else { b"false" });
    }
    if let Some(val) = session {
        i = copy(out, i, b",\"session\":");
        i = copy(out, i, val);
    }
    i = copy(out, i, b",\"evaluation_trace\":");
    i = copy(out, i, evaluation_trace);
    i = copy(out, i, b"}");
    i
}
fn trim_c_str(buf: &[u8]) -> &[u8] {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    if let Some(start) = buf.iter().position(|&b| b == b'{') {
        let mut depth = 0i32;
        let mut in_str = false;
        let mut i = start;
        while i < end {
            let b = buf[i];
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
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &buf[start..=i];
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }
    &buf[..end]
}

fn extract_object_at_depth<'a>(hay: &'a [u8], key: &[u8], depth: i32) -> Option<&'a [u8]> {
    let pos = find_key_at_depth(hay, key, depth)?;
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
    let mut depth_obj = 0i32;
    let mut in_str = false;
    let mut i = 0usize;
    while i < rest.len() {
        let b = rest[i];
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
            b'{' => depth_obj += 1,
            b'}' => {
                depth_obj -= 1;
                if depth_obj == 0 {
                    return Some(&rest[..=i]);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn extract_array_at_depth<'a>(hay: &'a [u8], key: &[u8], depth: i32) -> Option<&'a [u8]> {
    let pos = find_key_at_depth(hay, key, depth)?;
    let after = &hay[pos + key.len()..];
    let colon = after.iter().position(|b| *b == b':')?;
    let mut rest = &after[colon + 1..];
    while rest.first() == Some(&b' ')
        || rest.first() == Some(&b'\n')
        || rest.first() == Some(&b'\t')
    {
        rest = &rest[1..];
    }
    if rest.first() != Some(&b'[') {
        return None;
    }
    let mut depth_arr = 0i32;
    let mut in_str = false;
    let mut i = 0usize;
    while i < rest.len() {
        let b = rest[i];
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
            b'[' => depth_arr += 1,
            b']' => {
                depth_arr -= 1;
                if depth_arr == 0 {
                    return Some(&rest[..=i]);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
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

fn copy_usize(out: &mut [u8], at: usize, mut n: usize) -> usize {
    if n == 0 {
        return copy(out, at, b"0");
    }
    let mut digits = [0u8; 24];
    let mut d = 0usize;
    while n > 0 {
        digits[d] = b'0' + (n % 10) as u8;
        d += 1;
        n /= 10;
    }
    let mut i = at;
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

    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn run(input: &str) -> String {
        let _guard = TEST_LOCK.lock().unwrap();
        TEST_HOST_RC.with(|c| *c.borrow_mut() = 0);
        let mut out = vec![0u8; 65536];
        let n = unsafe { evaluate(input.as_bytes(), &mut out) };
        String::from_utf8_lossy(&out[..n]).into_owned()
    }

    fn set_get_response(json: &str) {
        TEST_GET_RESPONSE.with(|r| *r.borrow_mut() = json.as_bytes().to_vec());
    }

    #[test]
    fn happy_put() {
        let out = run(r#"{"action":"put","session":{"session_id":"sess-1","principal_id":"prin-1","scopes":["read","write"],"issued_at":"2026-09-08T20:00:00Z"}}"#);
        assert!(out.contains("\"reason_code\":\"ok\""), "{out}");
        assert!(out.contains("\"accepted\":true"));
        assert!(out.contains("\"session\":{"));
        assert!(out.contains("\"session_id\":\"sess-1\""));
    }

    #[test]
    fn happy_put_empty_scopes() {
        let out = run(r#"{"action":"put","session":{"session_id":"sess-1","principal_id":"prin-1","scopes":[],"issued_at":"2026-09-08T20:00:00Z"}}"#);
        assert!(out.contains("\"reason_code\":\"ok\""), "{out}");
    }

    #[test]
    fn rejects_put_missing_scopes() {
        let out = run(r#"{"action":"put","session":{"session_id":"sess-1","principal_id":"prin-1","issued_at":"2026-09-08T20:00:00Z"}}"#);
        assert!(out.contains("\"reason_code\":\"invalid_input\""), "{out}");
    }

    #[test]
    fn happy_get_found() {
        set_get_response(r#"{"found":true,"value":{"session_id":"sess-1","principal_id":"prin-1","scopes":["read","write"],"issued_at":"2026-09-08T20:00:00Z"}}"#);
        let out = run(r#"{"action":"get"}"#);
        assert!(out.contains("\"reason_code\":\"ok\""), "{out}");
        assert!(out.contains("\"found\":true"));
        assert!(out.contains("\"session_id\":\"sess-1\""));
    }

    #[test]
    fn get_not_found() {
        set_get_response(r#"{"found":false}"#);
        let out = run(r#"{"action":"get"}"#);
        assert!(out.contains("\"reason_code\":\"not_found\""), "{out}");
        assert!(out.contains("\"found\":false"));
        assert!(out.contains("\"accepted\":false"));
    }

    #[test]
    fn happy_delete() {
        let out = run(r#"{"action":"delete"}"#);
        assert!(out.contains("\"reason_code\":\"ok\""), "{out}");
        assert!(out.contains("\"accepted\":true"));
    }

    #[test]
    fn rejects_missing_action() {
        let out = run(r#"{"session":{"session_id":"x"}}"#);
        assert!(out.contains("\"reason_code\":\"invalid_input\""), "{out}");
    }

    #[test]
    fn rejects_invalid_action() {
        let out = run(r#"{"action":"list"}"#);
        assert!(out.contains("\"reason_code\":\"invalid_action\""), "{out}");
    }

    #[test]
    fn rejects_put_without_session() {
        let out = run(r#"{"action":"put"}"#);
        assert!(out.contains("\"reason_code\":\"invalid_input\""), "{out}");
    }

    #[test]
    fn rejects_put_missing_fields() {
        let out = run(r#"{"action":"put","session":{"session_id":"sess-1","principal_id":"","scopes":["read","write"],"issued_at":"2026-09-08T20:00:00Z"}}"#);
        assert!(out.contains("\"reason_code\":\"invalid_input\""), "{out}");
    }

    #[test]
    fn put_host_error() {
        let _guard = TEST_LOCK.lock().unwrap();
        TEST_HOST_RC.with(|c| *c.borrow_mut() = -2);
        let mut out = vec![0u8; 4096];
        let n = unsafe {
            evaluate(
                br#"{"action":"put","session":{"session_id":"sess-1","principal_id":"prin-1","scopes":["read","write"],"issued_at":"2026-09-08T20:00:00Z"}}"#,
                &mut out,
            )
        };
        let s = String::from_utf8_lossy(&out[..n]);
        assert!(s.contains("\"reason_code\":\"host_error\""), "{s}");
    }

    #[test]
    fn delete_host_error() {
        let _guard = TEST_LOCK.lock().unwrap();
        TEST_HOST_RC.with(|c| *c.borrow_mut() = -3);
        let mut out = vec![0u8; 4096];
        let n = unsafe { evaluate(br#"{"action":"delete"}"#, &mut out) };
        let s = String::from_utf8_lossy(&out[..n]);
        assert!(s.contains("\"reason_code\":\"host_error\""), "{s}");
    }

    #[test]
    fn get_host_error() {
        let _guard = TEST_LOCK.lock().unwrap();
        TEST_HOST_RC.with(|c| *c.borrow_mut() = -4);
        let mut out = vec![0u8; 4096];
        let n = unsafe { evaluate(br#"{"action":"get"}"#, &mut out) };
        let s = String::from_utf8_lossy(&out[..n]);
        assert!(s.contains("\"reason_code\":\"host_error\""), "{s}");
    }

    #[test]
    fn get_found_without_value_object() {
        set_get_response(r#"{"found":true,"value":"not-an-object"}"#);
        let out = run(r#"{"action":"get"}"#);
        assert!(out.contains("\"reason_code\":\"host_error\""), "{out}");
    }

    #[test]
    fn helpers_edge_cases() {
        assert_eq!(extract_i64(b"\"out_ptr\": 42", b"\"out_ptr\""), Some(42));
        assert_eq!(extract_i64(b"\"out_ptr\": -7", b"\"out_ptr\""), Some(-7));
        assert_eq!(extract_i64(b"\"out_ptr\":", b"\"out_ptr\""), None);
        assert_eq!(extract_i64(b"\"out_ptr\": true", b"\"out_ptr\""), None);
        assert_eq!(extract_i64(b"\"out_ptr\":\n\t 9", b"\"out_ptr\""), Some(9));
        assert_eq!(string_value_after(b": \"ok\""), b"ok");
        assert_eq!(string_value_after(b":\n\t\"ok\""), b"ok");
        assert_eq!(string_value_after(b"no"), b"");
        assert_eq!(string_value_after(b":not"), b"");
        assert_eq!(string_value_after(b":\"unterminated"), b"");
        let mut tiny = [0u8; 1];
        assert_eq!(copy(&mut tiny, 0, b"ab"), 0);
        let mut buf = [0u8; 8];
        let n0 = copy_usize(&mut buf, 0, 0);
        assert_eq!(&buf[..n0], b"0");
        let n1 = copy_usize(&mut buf, 0, 123);
        assert_eq!(&buf[..n1], b"123");
        let mut tiny2 = [0u8; 1];
        assert_eq!(copy_usize(&mut tiny2, 0, 12), 0);
        assert!(extract_object_at_depth(br#"{"session":null}"#, b"\"session\"", 1).is_none());
        assert!(extract_object_at_depth(br#"{"session":"x"}"#, b"\"session\"", 1).is_none());
        assert_eq!(
            extract_object_at_depth(br#"{"session":{"session_id":"x"}}"#, b"\"session\"", 1),
            Some(br#"{"session_id":"x"}"#.as_slice())
        );
        assert_eq!(
            extract_object_at_depth(
                br#"{"session":{"note":"a\"b","session_id":"x"}}"#,
                b"\"session\"",
                1
            ),
            Some(br#"{"note":"a\"b","session_id":"x"}"#.as_slice())
        );
        assert_eq!(
            extract_object_at_depth(br#"{"session": {"session_id":"z"}}"#, b"\"session\"", 1),
            Some(br#"{"session_id":"z"}"#.as_slice())
        );
        assert_eq!(
            extract_object_at_depth(br#"{"session":{"a":{"b":1}}}"#, b"\"session\"", 1),
            Some(br#"{"a":{"b":1}}"#.as_slice())
        );
        assert!(extract_object_at_depth(br#"{"session":{"session_id":"x""#, b"\"session\"", 1)
            .is_none());

        assert_eq!(
            extract_array_at_depth(br#"{"scopes":["a","b"]}"#, b"\"scopes\"", 1),
            Some(br#"["a","b"]"#.as_slice())
        );
        assert_eq!(
            extract_array_at_depth(br#"{"scopes":[]}"#, b"\"scopes\"", 1),
            Some(br#"[]"#.as_slice())
        );
        assert_eq!(
            extract_array_at_depth(br#"{"scopes": ["a"]}"#, b"\"scopes\"", 1),
            Some(br#"["a"]"#.as_slice())
        );
        assert_eq!(
            extract_array_at_depth(br#"{"scopes":["a\"b","c"]}"#, b"\"scopes\"", 1),
            Some(br#"["a\"b","c"]"#.as_slice())
        );
        assert!(extract_array_at_depth(br#"{"scopes":"x"}"#, b"\"scopes\"", 1).is_none());
        assert!(extract_array_at_depth(br#"{"scopes":["a""#, b"\"scopes\"", 1).is_none());

        assert_eq!(trim_c_str(b"{\"a\":1}\0xx"), br#"{"a":1}"#);
        assert_eq!(trim_c_str(b"no-brace\0"), b"no-brace");
        assert_eq!(trim_c_str(br#"{"a":"b\"c"}"#), br#"{"a":"b\"c"}"#);
        let nested = br#"{"wrap":{"x":1},"action":"get"}"#;
        assert_eq!(extract_string_at_depth(nested, b"\"action\"", 1), b"get");
        let with_arr = br#"{"tags":["a","b"],"action":"get"}"#;
        assert_eq!(extract_string_at_depth(with_arr, b"\"action\"", 1), b"get");
        let with_esc = br#"{"note":"say \"hi\"","action":"get"}"#;
        assert_eq!(extract_string_at_depth(with_esc, b"\"action\"", 1), b"get");
    }

    #[test]
    fn get_response_exact_buffer_len() {
        set_get_response(r#"{"found":false}"#);
        let out = run(r#"{"action":"get"}"#);
        assert!(out.contains("not_found"), "{out}");
    }

    #[test]
    fn state_get_oversized_response_returns_host_error() {
        let _guard = TEST_LOCK.lock().unwrap();
        TEST_GET_RESPONSE.with(|r| *r.borrow_mut() = vec![b'x'; 5000]);
        TEST_HOST_RC.with(|c| *c.borrow_mut() = 0);
        let mut out = vec![0u8; 4096];
        let n = unsafe { evaluate(br#"{"action":"get"}"#, &mut out) };
        let s = String::from_utf8_lossy(&out[..n]);
        assert!(s.contains("\"reason_code\":\"host_error\""), "{s}");
    }
}
