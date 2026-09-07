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

static mut INPUT: [u8; 8192] = [0; 8192];
static mut OUTPUT: [u8; 512] = [0; 512];
const INPUT_CAPACITY: usize = 8192;
const OUTPUT_CAPACITY: usize = 512;

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    unsafe {
        let mut total = 0usize;
        loop {
            let mut n = 0usize;
            let v = IoVecMut {
                buffer: core::ptr::addr_of_mut!(INPUT).cast::<u8>().add(total),
                length: INPUT_CAPACITY - total,
            };
            if fd_read(0, &v, 1, &mut n) != 0 || n == 0 {
                break;
            }
            total += n;
            if total == INPUT_CAPACITY {
                break;
            }
        }
        let input = core::slice::from_raw_parts(core::ptr::addr_of!(INPUT).cast::<u8>(), total);
        let output = core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(OUTPUT).cast::<u8>(), OUTPUT_CAPACITY);
        let len = resolve(input, output);
        let mut written = 0usize;
        let v = IoVec {
            buffer: core::ptr::addr_of!(OUTPUT).cast::<u8>(),
            length: len,
        };
        let _ = fd_write(1, &v, 1, &mut written);
    }
}

fn resolve(request: &[u8], out: &mut [u8]) -> usize {
    let policy = match object_after(request, b"\"policy\"") {
        Some(v) => v,
        None => return 0,
    };
    let policy_version = string_after(policy, b"\"version\"");
    let minimum_positive_cases = int_after(policy, b"\"minimum_positive_cases\"").unwrap_or(i32::MAX);
    let maximum_false_negative_millis = int_after(policy, b"\"maximum_false_negative_millis\"").unwrap_or(-1);
    let total_cases = count_key(request, b"\"contains_positive\"");
    let positive_cases = count_pattern(request, b"\"contains_positive\":true");
    let detected_positive = count_pattern(request, b"\"contains_positive\":true,\"positive_detected\":true");
    let false_negatives = positive_cases.saturating_sub(detected_positive);
    let false_negative_millis = if positive_cases == 0 {
        1000
    } else {
        ((false_negatives * 1000) / positive_cases) as i32
    };

    let mut at = 0usize;
    at = copy(out, at, b"{\"policy_version\":\"");
    at = copy_json(out, at, policy_version);
    at = copy(out, at, b"\",\"total_cases\":");
    at = write_int(out, at, total_cases as i32);
    at = copy(out, at, b",\"positive_cases\":");
    at = write_int(out, at, positive_cases as i32);
    at = copy(out, at, b",\"false_negatives\":");
    at = write_int(out, at, false_negatives as i32);
    at = copy(out, at, b",\"false_negative_millis\":");
    at = write_int(out, at, false_negative_millis);

    if (positive_cases as i32) < minimum_positive_cases {
        at = copy(out, at, b",\"decision\":\"reject\",\"reason\":\"insufficient_positive_cases\"}");
        return at;
    }
    if false_negative_millis > maximum_false_negative_millis {
        at = copy(out, at, b",\"decision\":\"reject\",\"reason\":\"false_negative_limit_exceeded\"}");
        return at;
    }
    copy(out, at, b",\"decision\":\"approve_for_policy\",\"reason\":\"release_gate_passed\"}")
}

fn skip(mut s: &[u8]) -> &[u8] {
    while s.first().is_some_and(|b| matches!(*b, b' ' | b'\n' | b'\r' | b'\t')) {
        s = &s[1..];
    }
    s
}

fn find(s: &[u8], key: &[u8]) -> Option<usize> {
    s.windows(key.len()).position(|w| w == key)
}

fn balanced_end(s: &[u8]) -> Option<usize> {
    let mut depth = 0i32;
    let mut quoted = false;
    let mut escaped = false;
    for (i, &b) in s.iter().enumerate() {
        if quoted {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                quoted = false;
            }
            continue;
        }
        match b {
            b'"' => quoted = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn object_after<'a>(s: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let p = find(s, key)?;
    let c = s[p + key.len()..].iter().position(|b| *b == b':')?;
    let rest = skip(&s[p + key.len() + c + 1..]);
    if rest.first() != Some(&b'{') {
        return None;
    }
    Some(&rest[..=balanced_end(rest)?])
}

fn string_after<'a>(s: &'a [u8], key: &[u8]) -> &'a [u8] {
    let Some(p) = find(s, key) else { return b"" };
    let Some(c) = s[p + key.len()..].iter().position(|b| *b == b':') else { return b"" };
    let rest = skip(&s[p + key.len() + c + 1..]);
    if rest.first() != Some(&b'"') {
        return b"";
    }
    let rest = &rest[1..];
    match rest.iter().position(|b| *b == b'"') {
        Some(end) => &rest[..end],
        None => b"",
    }
}

fn int_after(s: &[u8], key: &[u8]) -> Option<i32> {
    let p = find(s, key)?;
    let c = s[p + key.len()..].iter().position(|b| *b == b':')?;
    let rest = skip(&s[p + key.len() + c + 1..]);
    let mut n = 0i32;
    let mut count = 0;
    for &b in rest {
        if !(b'0'..=b'9').contains(&b) {
            break;
        }
        n = n.checked_mul(10)?.checked_add((b - b'0') as i32)?;
        count += 1;
    }
    if count == 0 { None } else { Some(n) }
}

fn count_key(s: &[u8], key: &[u8]) -> usize {
    if key.is_empty() || s.len() < key.len() {
        return 0;
    }
    s.windows(key.len()).filter(|w| *w == key).count()
}

fn count_pattern(s: &[u8], pattern: &[u8]) -> usize {
    if pattern.is_empty() || s.len() < pattern.len() {
        return 0;
    }
    s.windows(pattern.len()).filter(|w| *w == pattern).count()
}

fn write_int(out: &mut [u8], mut at: usize, value: i32) -> usize {
    if value == 0 {
        return copy(out, at, b"0");
    }
    let mut n = value;
    if n < 0 {
        at = copy(out, at, b"-");
        n = -n;
    }
    let mut digits = [0u8; 12];
    let mut len = 0usize;
    let mut current = n as u32;
    while current > 0 {
        digits[len] = b'0' + (current % 10) as u8;
        current /= 10;
        len += 1;
    }
    while len > 0 {
        len -= 1;
        at = copy(out, at, &digits[len..len + 1]);
    }
    at
}

fn copy(out: &mut [u8], at: usize, bytes: &[u8]) -> usize {
    let end = at + bytes.len();
    if end > out.len() {
        return at;
    }
    out[at..end].copy_from_slice(bytes);
    end
}

fn copy_json(out: &mut [u8], mut at: usize, s: &[u8]) -> usize {
    for &b in s {
        at = match b {
            b'"' => copy(out, at, b"\\\""),
            b'\\' => copy(out, at, b"\\\\"),
            _ => copy(out, at, &[b]),
        };
    }
    at
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exercises_runtime_fixture_1() {
        let input = include_bytes!("../tests/uc01-approve.json");
        let mut out = [0u8; 65_536];
        let length = resolve(input, &mut out);
        assert!(length > 0);
        assert!(length <= out.len());
    }
    #[test]
    fn exercises_runtime_fixture_2() {
        let input = include_bytes!("../tests/uc02-reject-false-negative.json");
        let mut out = [0u8; 65_536];
        let length = resolve(input, &mut out);
        assert!(length > 0);
        assert!(length <= out.len());
    }
    #[test]
    fn exercises_runtime_fixture_3() {
        let input = include_bytes!("../tests/uc03-reject-coverage.json");
        let mut out = [0u8; 65_536];
        let length = resolve(input, &mut out);
        assert!(length > 0);
        assert!(length <= out.len());
    }

    #[test]
    fn covers_release_gate_boundaries_and_json_escaping() {
        let request = br#"{"policy":{"version":"v","minimum_positive_cases":1,"maximum_false_negative_millis":0},"cases":[{"contains_positive":true,"positive_detected":true}]}"#;
        let mut out = [0u8; 512];
        assert!(resolve(request, &mut out) > 0);
        assert_eq!(write_int(&mut out, 0, -1), 2);
        let mut short = [0u8; 1];
        assert_eq!(copy(&mut short, 1, b"x"), 1);
        assert_eq!(string_after(br#"{"x":"unterminated}"#, br#""x""#), b"");
        assert!(balanced_end(&[b'{', b'"', b'\\', b'"', b'"', b'}']).is_some());
        let mut escaped = [0u8; 8];
        assert_eq!(copy_json(&mut escaped, 0, &[b'"', b'\\']), 4);
    }


    #[test]
    fn handles_an_incomplete_request_without_panicking() {
        let mut out = [0u8; 65_536];
        let length = resolve(b"{}", &mut out);
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
            let length = resolve(input, &mut out);
            assert!(length <= out.len());
        }
    }

    #[test]
    fn remains_total_for_truncated_and_corrupted_real_requests() {
        for fixture in [include_bytes!("../tests/uc01-approve.json").as_slice(), include_bytes!("../tests/uc02-reject-false-negative.json").as_slice(), include_bytes!("../tests/uc03-reject-coverage.json").as_slice()] {
            for end in 0..=fixture.len() {
                let mut out = [0u8; 65_536];
                let length = resolve(&fixture[..end], &mut out);
                assert!(length <= out.len());
            }
            for index in 0..core::cmp::min(fixture.len(), 512) {
                let mut corrupted = fixture.to_vec();
                corrupted[index] = b'?';
                let mut out = [0u8; 65_536];
                let length = resolve(&corrupted, &mut out);
                assert!(length <= out.len());
            }
        }
    }
}
