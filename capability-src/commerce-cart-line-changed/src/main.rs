//! commerce.cart-line-changed — validate a cart line quantity change and emit.
//!
//! Given cart_id, line_id, sku, qty_before, qty_after, actor_id: requires
//! non-empty identity fields and qty_* >= 0. On success emits
//! commerce.cart.line-changed@1.0.0 via traverse_host::emit_event.
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
static mut EVENT_BUF: [u8; 1536] = [0; 1536];

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
    let cart_id = extract_string_at_depth(input, b"\"cart_id\"", 1);
    let line_id = extract_string_at_depth(input, b"\"line_id\"", 1);
    let sku = extract_string_at_depth(input, b"\"sku\"", 1);
    let actor_id = extract_string_at_depth(input, b"\"actor_id\"", 1);
    let qty_before = extract_i64(input, b"\"qty_before\"");
    let qty_after = extract_i64(input, b"\"qty_after\"");

    if cart_id.is_empty() || line_id.is_empty() || sku.is_empty() || actor_id.is_empty() {
        return write_result(
            out,
            false,
            0,
            0,
            b"invalid_input",
            br#"["precondition failed: required string fields missing"]"#,
        );
    }

    let (Some(before), Some(after)) = (qty_before, qty_after) else {
        return write_result(
            out,
            false,
            0,
            0,
            b"invalid_input",
            br#"["precondition failed: qty_before and qty_after required as integers"]"#,
        );
    };

    if before < 0 || after < 0 {
        return write_result(
            out,
            false,
            before,
            after,
            b"invalid_qty",
            br#"["qty_before and qty_after must be >= 0"]"#,
        );
    }

    if before == after {
        return write_result(
            out,
            false,
            before,
            after,
            b"no_change",
            br#"["qty_before equals qty_after; nothing to emit"]"#,
        );
    }

    emit_cart_line_changed(cart_id, line_id, sku, before, after, actor_id);

    let mut trace = [0u8; 160];
    let mut t = 0usize;
    t = copy(&mut trace, t, b"[\"");
    t = copy(&mut trace, t, line_id);
    t = copy(&mut trace, t, b" qty changed\"]");
    write_result(out, true, before, after, b"ok", &trace[..t])
}

unsafe fn emit_cart_line_changed(
    cart_id: &[u8],
    line_id: &[u8],
    sku: &[u8],
    qty_before: i64,
    qty_after: i64,
    actor_id: &[u8],
) {
    let buf = &mut EVENT_BUF;
    let mut i = 0usize;
    i = copy(
        buf,
        i,
        br#"{"event_id":"commerce.cart.line-changed","version":"1.0.0","payload":{"cart_id":""#,
    );
    i = copy(buf, i, cart_id);
    i = copy(buf, i, br#"","line_id":""#);
    i = copy(buf, i, line_id);
    i = copy(buf, i, br#"","sku":""#);
    i = copy(buf, i, sku);
    i = copy(buf, i, br#"","qty_before":"#);
    i = copy_i64(buf, i, qty_before);
    i = copy(buf, i, br#","qty_after":"#);
    i = copy_i64(buf, i, qty_after);
    i = copy(buf, i, br#","actor_id":""#);
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
    qty_before: i64,
    qty_after: i64,
    reason_code: &[u8],
    evaluation_trace: &[u8],
) -> usize {
    let mut i = 0usize;
    i = copy(out, i, b"{\"accepted\":");
    i = copy(out, i, if accepted { b"true" } else { b"false" });
    i = copy(out, i, b",\"qty_before\":");
    i = copy_i64(out, i, qty_before);
    i = copy(out, i, b",\"qty_after\":");
    i = copy_i64(out, i, qty_after);
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
    fn happy_qty_increase() {
        let out = run(
            r#"{"cart_id":"cart-1","line_id":"line-1","sku":"SKU-A","qty_before":1,"qty_after":3,"actor_id":"user-ada"}"#,
        );
        assert!(out.contains("\"reason_code\":\"ok\""), "{out}");
        assert!(out.contains("\"accepted\":true"));
    }

    #[test]
    fn happy_qty_decrease_to_zero() {
        let out = run(
            r#"{"cart_id":"cart-1","line_id":"line-1","sku":"SKU-A","qty_before":2,"qty_after":0,"actor_id":"user-ada"}"#,
        );
        assert!(out.contains("\"reason_code\":\"ok\""), "{out}");
    }

    #[test]
    fn rejects_negative_qty() {
        let out = run(
            r#"{"cart_id":"cart-1","line_id":"line-1","sku":"SKU-A","qty_before":1,"qty_after":-1,"actor_id":"user-ada"}"#,
        );
        assert!(out.contains("\"reason_code\":\"invalid_qty\""), "{out}");
        assert!(out.contains("\"accepted\":false"));
    }

    #[test]
    fn rejects_no_change() {
        let out = run(
            r#"{"cart_id":"cart-1","line_id":"line-1","sku":"SKU-A","qty_before":2,"qty_after":2,"actor_id":"user-ada"}"#,
        );
        assert!(out.contains("\"reason_code\":\"no_change\""), "{out}");
    }

    #[test]
    fn rejects_missing_fields() {
        let out = run(r#"{"cart_id":"cart-1"}"#);
        assert!(out.contains("\"reason_code\":\"invalid_input\""), "{out}");
    }

    #[test]
    fn rejects_missing_qty() {
        let out =
            run(r#"{"cart_id":"cart-1","line_id":"line-1","sku":"SKU-A","actor_id":"user-ada"}"#);
        assert!(out.contains("\"reason_code\":\"invalid_input\""), "{out}");
    }

    #[test]
    fn rejects_qty_null_token() {
        let out = run(
            r#"{"cart_id":"cart-1","line_id":"line-1","sku":"SKU-A","qty_before":null,"qty_after":1,"actor_id":"user-ada"}"#,
        );
        assert!(out.contains("\"reason_code\":\"invalid_input\""), "{out}");
    }

    #[test]
    fn parses_qty_with_whitespace_and_negative() {
        assert_eq!(
            extract_i64(b"\"qty_before\":\n\t 12", b"\"qty_before\""),
            Some(12)
        );
        assert_eq!(
            extract_i64(b"\"qty_before\": -7", b"\"qty_before\""),
            Some(-7)
        );
        assert_eq!(extract_i64(b"\"qty_before\":", b"\"qty_before\""), None);
        assert_eq!(
            extract_i64(b"\"qty_before\": true", b"\"qty_before\""),
            None
        );
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
        let with_escape = br#"{"note":"say \"hi\"","cart_id":"c1","line_id":"l1","sku":"s","qty_before":1,"qty_after":2,"actor_id":"a"}"#;
        assert_eq!(
            extract_string_at_depth(with_escape, b"\"cart_id\"", 1),
            b"c1"
        );
        let nested = br#"{"wrap":{"x":1},"cart_id":"c2","line_id":"l2","sku":"s","qty_before":1,"qty_after":2,"actor_id":"a"}"#;
        assert_eq!(extract_string_at_depth(nested, b"\"cart_id\"", 1), b"c2");
        let with_array = br#"{"tags":["a","b"],"cart_id":"c3"}"#;
        assert_eq!(
            extract_string_at_depth(with_array, b"\"cart_id\"", 1),
            b"c3"
        );
    }

    #[test]
    fn copy_overflow_returns_at() {
        let mut tiny = [0u8; 2];
        assert_eq!(copy(&mut tiny, 0, b"hello"), 0);
    }
}
