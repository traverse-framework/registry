#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

#[repr(C)]
struct IoVec { buffer: *const u8, length: usize }
#[repr(C)]
struct IoVecMut { buffer: *mut u8, length: usize }
#[cfg(not(test))]
#[link(wasm_import_module = "wasi_snapshot_preview1")]
unsafe extern "C" {
    fn fd_read(fd: u32, vectors: *const IoVecMut, count: usize, read: *mut usize) -> u32;
    fn fd_write(fd: u32, vectors: *const IoVec, count: usize, written: *mut usize) -> u32;
}
static mut INPUT: [u8; 8192] = [0; 8192];
static mut OUTPUT: [u8; 512] = [0; 512];

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() { unsafe {
    let mut total = 0usize;
    loop {
        let mut n = 0usize;
        let v = IoVecMut { buffer: INPUT.as_mut_ptr().add(total), length: INPUT.len() - total };
        if fd_read(0, &v, 1, &mut n) != 0 || n == 0 { break; }
        total += n;
        if total == INPUT.len() { break; }
    }
    let len = score(&INPUT[..total], &mut OUTPUT);
    let mut written = 0usize;
    let v = IoVec { buffer: OUTPUT.as_ptr(), length: len };
    let _ = fd_write(1, &v, 1, &mut written);
} }

fn score(request: &[u8], out: &mut [u8]) -> usize {
    let included_count = int_after(request, b"\"included_count\"").unwrap_or(0);
    let pending_count = int_after(request, b"\"pending_count\"").unwrap_or(0);
    let policy = match object_after(request, b"\"policy\"") { Some(v) => v, None => return error_json(out, b"policy.version is required") };
    let policy_version = string_after(policy, b"\"version\"");
    if policy_version.is_empty() { return error_json(out, b"policy.version is required"); }

    let total = included_count + pending_count;
    let (uncertainty_millis, reason_code): (i32, &[u8]) =
        if total == 0 { (1000, b"empty_period") }
        else { (((1000 * pending_count) / total), b"pending_fraction") };

    let mut at = 0usize;
    at = copy(out, at, b"{\"uncertainty_millis\":");
    at = write_i32(out, at, uncertainty_millis);
    at = copy(out, at, b",\"reason_code\":\"");
    at = copy(out, at, reason_code);
    at = copy(out, at, b"\",\"policy_version\":\"");
    at = copy_json(out, at, policy_version);
    copy(out, at, b"\"}")
}

fn error_json(out: &mut [u8], message: &[u8]) -> usize {
    let mut at = 0usize;
    at = copy(out, at, b"{\"error\":\"");
    at = copy_json(out, at, message);
    copy(out, at, b"\"}")
}
fn skip(mut s: &[u8]) -> &[u8] {
    while s.first().is_some_and(|b| matches!(*b, b' ' | b'\n' | b'\r' | b'\t')) { s = &s[1..]; }
    s
}
fn find(s: &[u8], key: &[u8]) -> Option<usize> { s.windows(key.len()).position(|w| w == key) }
fn balanced_end(s: &[u8]) -> Option<usize> {
    let mut depth = 0i32; let mut quoted = false; let mut escaped = false;
    for (i, &b) in s.iter().enumerate() {
        if quoted {
            if escaped { escaped = false; } else if b == b'\\' { escaped = true; } else if b == b'"' { quoted = false; }
            continue;
        }
        match b {
            b'"' => quoted = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => { depth -= 1; if depth == 0 { return Some(i); } }
            _ => {}
        }
    }
    None
}
fn object_after<'a>(s: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let p = find(s, key)?;
    let c = s[p + key.len()..].iter().position(|b| *b == b':')?;
    let rest = skip(&s[p + key.len() + c + 1..]);
    if rest.first() != Some(&b'{') { return None; }
    Some(&rest[..=balanced_end(rest)?])
}
fn string_after<'a>(s: &'a [u8], key: &[u8]) -> &'a [u8] {
    let Some(p) = find(s, key) else { return b"" };
    let Some(c) = s[p + key.len()..].iter().position(|b| *b == b':') else { return b"" };
    let rest = skip(&s[p + key.len() + c + 1..]);
    if rest.first() != Some(&b'"') { return b""; }
    let rest = &rest[1..];
    match rest.iter().position(|b| *b == b'"') { Some(end) => &rest[..end], None => b"" }
}
fn int_after(s: &[u8], key: &[u8]) -> Option<i32> {
    let p = find(s, key)?;
    let c = s[p + key.len()..].iter().position(|b| *b == b':')?;
    let r = skip(&s[p + key.len() + c + 1..]);
    let (mut n, mut count) = (0i32, 0);
    for &b in r {
        if !(b'0'..=b'9').contains(&b) { break; }
        n = n.checked_mul(10)?.checked_add((b - b'0') as i32)?;
        count += 1;
    }
    if count == 0 { None } else { Some(n) }
}
fn copy(out: &mut [u8], at: usize, bytes: &[u8]) -> usize {
    let end = at + bytes.len();
    if end > out.len() { return at; }
    out[at..end].copy_from_slice(bytes);
    end
}
fn copy_json(out: &mut [u8], mut at: usize, s: &[u8]) -> usize {
    for &b in s {
        at = match b { b'"' => copy(out, at, b"\\\""), b'\\' => copy(out, at, b"\\\\"), _ => copy(out, at, &[b]) };
    }
    at
}
fn write_i32(out: &mut [u8], mut at: usize, mut n: i32) -> usize {
    if n == 0 { return copy(out, at, b"0"); }
    let mut digits = [0u8; 10];
    let mut len = 0usize;
    while n > 0 { digits[len] = b'0' + (n % 10) as u8; n /= 10; len += 1; }
    while len > 0 { len -= 1; at = copy(out, at, &[digits[len]]); }
    at
}
#[cfg(not(test))]
#[panic_handler] fn panic(_: &core::panic::PanicInfo<'_>) -> ! { loop {} }


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exercises_runtime_fixture_1() {
        let input = include_bytes!("../tests/uc01-pending-fraction.json");
        let mut out = [0u8; 65_536];
        let length = score(input, &mut out);
        assert!(length > 0);
        assert!(length <= out.len());
    }
    #[test]
    fn exercises_runtime_fixture_2() {
        let input = include_bytes!("../tests/uc02-empty-period.json");
        let mut out = [0u8; 65_536];
        let length = score(input, &mut out);
        assert!(length > 0);
        assert!(length <= out.len());
    }


    #[test]
    fn handles_an_incomplete_request_without_panicking() {
        let mut out = [0u8; 65_536];
        let length = score(b"{}", &mut out);
        assert!(length <= out.len());
    }

    #[test]
    fn rejects_malformed_json_shapes_without_panicking() {
        for input in [
            b"".as_slice(), b"{".as_slice(), b"[]".as_slice(), b"null".as_slice(),
            br#"{"x":null}"#.as_slice(), br#"{"x":[]}"#.as_slice(),
            br#"{"x":{}}"#.as_slice(), br#"{"items":[]}"#.as_slice(),
            br#"{"policy":{}}"#.as_slice(), br#"{"location":{}}"#.as_slice(),
        ] {
            let mut out = [0u8; 65_536];
            let length = score(input, &mut out);
            assert!(length <= out.len());
        }
    }

    #[test]
    fn remains_total_for_truncated_and_corrupted_real_requests() {
        for fixture in [include_bytes!("../tests/uc01-pending-fraction.json").as_slice(), include_bytes!("../tests/uc02-empty-period.json").as_slice()] {
            for end in 0..=fixture.len() {
                let mut out = [0u8; 65_536];
                let length = score(&fixture[..end], &mut out);
                assert!(length <= out.len());
            }
            for index in 0..core::cmp::min(fixture.len(), 512) {
                let mut corrupted = fixture.to_vec();
                corrupted[index] = b'?';
                let mut out = [0u8; 65_536];
                let length = score(&corrupted, &mut out);
                assert!(length <= out.len());
            }
        }
    }
}
